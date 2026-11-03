#!/usr/bin/env python3
"""CI-only genuine synthetic builds for the installed version lifecycle gate.

Never publishes a version or changes candidate assets. A/C are actual isolated
commits, not relabelled candidate binaries. Python is not a product dependency.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import stat
import subprocess
import time

import marmish_native_https_check as checks

Refused, require, execute = checks.Refused, checks.require, checks.supervisor.execute
FILES = checks.FILES
SOURCE_FILES = ('crates/marmish-native/Cargo.toml', 'Cargo.lock',
                'crates/marmish-native/src/journal.rs')
VARIANTS = ('a', 'c')
TARGETS = {'x86_64-unknown-linux-gnu': ('Linux', 'x86_64'),
           'aarch64-unknown-linux-gnu': ('Linux', 'aarch64'),
           'aarch64-apple-darwin': ('Darwin', 'arm64')}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def cancellation_handlers():
    def cancelled(_signum, _frame):
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        raise Refused('lifecycle_build_signal')
    signal.signal(signal.SIGTERM, cancelled)
    signal.signal(signal.SIGINT, cancelled)


def regular(path, maximum, *, cargo_output=False):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and (cargo_output or info.st_nlink == 1)
            and info.st_size <= maximum, 'unsafe_lifecycle_input')
    return path.read_bytes()


def private(path):
    path.mkdir(mode=0o700)
    require(path.resolve() == path and path.stat().st_uid == os.geteuid()
            and stat.S_IMODE(path.stat().st_mode) == 0o700, 'lifecycle_private_directory')
    return path


def candidate_version(original):
    """Derive one canonical stable native package version, including lock parity."""
    require(set(original) == set(SOURCE_FILES), 'variant_source_set')
    found = re.findall(r'(?m)^\[package\]\nname = "marmish-native"\nversion = "([^"\n]+)"\n',
                       original[SOURCE_FILES[0]])
    require(len(found) == 1, 'variant_version_patch')
    version = checks.version_value(found[0])
    require('-' not in version and len(version) <= 43, 'variant_version_patch')
    entries = re.findall(r'(?m)^\[\[package\]\]\nname = "marmish-native"\nversion = "([^"\n]+)"\n',
                         original[SOURCE_FILES[1]])
    require(entries == [version], 'variant_version_patch')
    return version


def variant_versions(version):
    return {'a': version + '-rc.1', 'c': version + '-rc.2'}


def patch_sources(original, variant):
    """Closed text replacements change only native package/version and C format."""
    require(variant in VARIANTS and set(original) == set(SOURCE_FILES), 'variant_source_set')
    version = candidate_version(original)
    output = dict(original)
    for name, prefix in ((SOURCE_FILES[0], '[package]\n'), (SOURCE_FILES[1], '[[package]]\n')):
        before = prefix + 'name = "marmish-native"\nversion = "' + version + '"\n'
        after = prefix + 'name = "marmish-native"\nversion = "' + variant_versions(version)[variant] + '"\n'
        require(output[name].count(before) == 1, 'variant_version_patch')
        output[name] = output[name].replace(before, after)
    journal = original[SOURCE_FILES[2]]
    formats = re.findall(r'pub\(crate\) const JOURNAL_FORMAT: i64 = ([0-9]+);', journal)
    supported = re.findall(r'pub\(crate\) const JOURNAL_SUPPORTED_FORMATS: &\[i64\] = &\[([0-9, ]+)\];', journal)
    require(len(formats) == len(supported) == 1 and int(formats[0]) > 4,
            'variant_candidate_format')
    values = [int(value.strip()) for value in supported[0].split(',')]
    require(len(set(values)) == len(values) and {0, 3, 4, int(formats[0])} <= set(values),
            'variant_supported_formats')
    if variant == 'c':
        output[SOURCE_FILES[2]] = journal.replace(
            'pub(crate) const JOURNAL_FORMAT: i64 = ' + formats[0] + ';',
            'pub(crate) const JOURNAL_FORMAT: i64 = 4;').replace(
            'pub(crate) const JOURNAL_SUPPORTED_FORMATS: &[i64] = &[' + supported[0] + '];',
            'pub(crate) const JOURNAL_SUPPORTED_FORMATS: &[i64] = &[0, 3, 4];')
    return output, int(formats[0])


def manifest_name(version, target):
    return f'marmish-{version}-{target}.manifest'


def inventory(directory, version, target):
    names = {f'{name}-{version}-{target}' for name in FILES}
    names.add(manifest_name(version, target))
    require(set(path.name for path in directory.iterdir()) == names, 'variant_asset_set')
    return {name: sha(regular(directory / name, 268435456)) for name in sorted(names)}


def verify_assets(directory, version, commit, target, scratch, expected_formats=None):
    """Reuse the shipped manifest checker, with independent fixed-name copies."""
    before = inventory(directory, version, target)
    raw = regular(directory / manifest_name(version, target), 4096)
    fixed = private(scratch)
    for name in FILES:
        shutil.copyfile(directory / f'{name}-{version}-{target}', fixed / name)
        (fixed / name).chmod(0o444 if name == FILES[-1] else 0o555)
    context = {'version': version, 'source_commit': commit,
               'manifest_pins': {target: sha(raw)}}
    formats = checks.check_manifest(raw, context, target, fixed)
    require(expected_formats is None or formats == expected_formats, 'variant_compiled_formats')
    require(inventory(directory, version, target) == before, 'candidate_assets_changed')
    return formats, before


def build_environment():
    names = ('PATH', 'HOME', 'USER', 'LOGNAME', 'TMPDIR', 'CARGO_HOME', 'RUSTUP_HOME',
             'SSL_CERT_FILE', 'SSL_CERT_DIR')
    env = {name: os.environ[name] for name in names if name in os.environ}
    env.update(LC_ALL='C', CARGO_TERM_COLOR='never', MARMISH_NATIVE_TOOLCHAIN='1',
               RUSTUP_TOOLCHAIN='1.97.1', GIT_CONFIG_NOSYSTEM='1',
               GIT_CONFIG_GLOBAL=os.devnull, GIT_TERMINAL_PROMPT='0')
    if platform.system() == 'Darwin':
        env['MACOSX_DEPLOYMENT_TARGET'] = '15.0'
    return env


class Build:
    def __init__(self, options):
        self.options = options
        self.env = build_environment()
        self.cargo = options.cargo
        require(self.cargo.is_absolute() and self.cargo.is_file()
                and os.access(self.cargo, os.X_OK), 'lifecycle_cargo')
        self.env['PATH'] = str(self.cargo.parent) + os.pathsep + self.env.get('PATH', '')
        # Original job deadline includes build, runtime, cleanup and transfer.
        remaining = min(options.deadline - time.time() - 2100,
                        options.build_deadline - time.time() - 30)
        require(remaining > 0, 'lifecycle_build_budget')
        self.deadline = time.monotonic() + min(2700, remaining)
        self.root = private(options.output.absolute())
        self.clone = self.root / 'source'
        self.target = self.root / 'build-target'
        self.env['CARGO_TARGET_DIR'] = str(self.target)
        self.stage = 'inputs'

    def run(self, argv, cap=2700, env=None):
        remaining = min(cap, self.deadline - time.monotonic())
        require(remaining > 0, 'lifecycle_build_budget')
        return execute([str(value) for value in argv], self.env if env is None else env,
                       timeout=remaining)[1]

    def git(self, *arguments):
        return self.run(['git', '-c', 'core.hooksPath=' + os.devnull, '-C', self.clone,
                         *arguments], cap=120).decode('utf-8').strip()

    def prepare(self):
        cargo = self.run([self.cargo, '--version'], cap=30)
        rustc = self.run([self.cargo.with_name('rustc'), '-vV'], cap=30)
        require(cargo.startswith(b'cargo 1.97.1 (')
                and b'\nrelease: 1.97.1\n' in rustc
                and ('\nhost: ' + self.options.target + '\n').encode() in rustc,
                'lifecycle_toolchain')
        repo = Path(__file__).resolve().parents[1]
        actual = self.run(['git', '-C', repo, 'rev-parse', 'HEAD'], cap=15).decode().strip()
        require(actual == self.options.source, 'lifecycle_source_commit')
        self.run(['git', '-C', repo, 'diff', '--quiet', 'HEAD', '--'], cap=15)
        # No shared object links, global config or inherited hooks/credentials.
        self.run(['git', '-c', 'core.hooksPath=' + os.devnull, 'clone', '--no-local',
                  '--no-hardlinks', '--no-checkout', repo, self.clone], cap=120)
        self.git('checkout', '--detach', self.options.source)
        self.original = {name: regular(self.clone / name, 1048576).decode('utf-8')
                         for name in SOURCE_FILES}
        self.version = candidate_version(self.original)
        patch_sources(self.original, 'a')
        timestamp = self.git('show', '-s', '--format=%ct', self.options.source)
        require(re.fullmatch(r'[0-9]{1,12}', timestamp), 'variant_commit_time')
        self.env.update(GIT_AUTHOR_NAME='Marmish lifecycle fixture',
                        GIT_COMMITTER_NAME='Marmish lifecycle fixture',
                        GIT_AUTHOR_EMAIL='lifecycle@example.invalid',
                        GIT_COMMITTER_EMAIL='lifecycle@example.invalid',
                        GIT_AUTHOR_DATE='@' + timestamp + ' +0000',
                        GIT_COMMITTER_DATE='@' + timestamp + ' +0000')

    def commit_variant(self, variant):
        self.git('checkout', '--detach', self.options.source)
        patched, _ = patch_sources(self.original, variant)
        for name in SOURCE_FILES:
            if patched[name] != self.original[name]:
                (self.clone / name).write_text(patched[name])
        expected = set(SOURCE_FILES[:2] if variant == 'a' else SOURCE_FILES)
        changed = set(self.git('diff', '--name-only', '--').splitlines())
        require(changed == expected, 'variant_patch_scope')
        patch = self.run(['git', '-c', 'core.hooksPath=' + os.devnull, '-C', self.clone,
                          'diff', '--no-ext-diff', '--binary', '--', *sorted(expected)], cap=30)
        require(len(patch) <= 16384, 'variant_patch_size')
        patch_path = self.root / 'patches' / (variant + '.patch')
        patch_path.write_bytes(patch)
        patch_path.chmod(0o600)
        self.git('add', '--', *sorted(expected))
        self.git('-c', 'commit.gpgsign=false', 'commit', '--no-verify', '--no-gpg-sign',
                 '-m', 'Synthetic lifecycle fixture ' + variant)
        commit = self.git('rev-parse', 'HEAD')
        require(self.git('rev-parse', 'HEAD^') == self.options.source
                and re.fullmatch(r'[0-9a-f]{40}', commit), 'variant_commit_parent')
        self.git('diff', '--quiet', 'HEAD', '--')
        return commit, self.git('rev-parse', 'HEAD^{tree}'), sha(patch)

    def compile_variant(self, variant):
        version = variant_versions(self.version)[variant]
        self.stage = 'build_' + variant
        started = time.monotonic()
        commit, tree, patch_hash = self.commit_variant(variant)
        env = dict(self.env, MARMISH_BUILD_COMMIT=commit)
        self.run([self.cargo, 'build', '--frozen', '--release', '--manifest-path',
                  self.clone / 'Cargo.toml', '-p', 'marmish-native', '--bins'], env=env)
        self.git('diff', '--quiet', 'HEAD', '--')
        inputs = private(self.root / ('input-' + variant))
        for name in FILES[:-1]:
            source = self.target / 'release' / name
            regular(source, 268435456, cargo_output=True)  # Copy normal Cargo hardlinks.
            shutil.copyfile(source, inputs / name)
            (inputs / name).chmod(0o755)
        signing, minimum = 'none', 'glibc=2.39'
        if self.options.target.endswith('apple-darwin'):
            signing, minimum = 'adhoc', 'macos=15.0'
            for name in FILES[:-1]:
                binary = inputs / name
                self.run(['/usr/bin/codesign', '--force', '--sign', '-', binary], cap=30)
                self.run(['/usr/bin/codesign', '--verify', '--strict', binary], cap=30)
                observed = self.run(['/bin/sh', '-c',
                    'exec /usr/bin/codesign -dv --verbose=4 "$1" 2>&1', 'sh', binary], cap=30)
                require(b'Signature=adhoc' in observed and b'Authority=' not in observed,
                        'variant_signing_kind')
        self.run(['/bin/sh', self.clone / 'scripts/v02-inspect-native.sh', inputs], cap=120)
        output = self.root / variant
        license_path = self.options.candidate_assets / f'THIRD-PARTY-LICENSES-{self.version}-{self.options.target}'
        self.run([self.target / 'release/marmish-release-tool', 'package',
                  '--input-dir', inputs, '--output-dir', output, '--licenses', license_path,
                  '--signing', signing, '--minimum', minimum], cap=120)
        expected = list(self.candidate_formats)
        if variant == 'c':
            expected[1] = 4
        formats, _ = verify_assets(output, version, commit, self.options.target,
                                   self.root / ('verify-' + variant), expected)
        for name in FILES[:-1]:
            raw = self.run([output / f'{name}-{version}-{self.options.target}',
                            'release-info'], cap=15)
            checks.release_contract(json.loads(raw), {'version': version,
                'source_commit': commit}, self.options.target, formats)
        return {'version': version, 'commit': commit, 'tree': tree,
                'patch_sha256': patch_hash, 'manifest_sha256': checks.supervisor.digest(
                    output / manifest_name(version, self.options.target)),
                'build_ms': int((time.monotonic() - started) * 1000), 'signing_kind': signing}

    def build(self):
        self.prepare()
        formats, original = verify_assets(self.options.candidate_assets, self.version,
            self.options.source, self.options.target, self.root / 'verify-b')
        require(formats[1] > 4, 'variant_candidate_format')
        self.candidate_formats = formats
        self.stage = 'dependencies'
        self.run([self.cargo, 'fetch', '--locked', '--manifest-path', self.clone / 'Cargo.toml'])
        self.git('diff', '--quiet', 'HEAD', '--')
        private(self.root / 'patches')
        result = {'schema': 1, 'source_commit': self.options.source,
                  'target': self.options.target,
                  'b_manifest_sha256': original[manifest_name(self.version, self.options.target)]}
        for variant in VARIANTS:
            result[variant] = self.compile_variant(variant)
            require(inventory(self.options.candidate_assets, self.version, self.options.target)
                    == original, 'candidate_assets_changed')
        require(self.options.deadline - time.time() >= 2100, 'lifecycle_runtime_budget')
        record = self.root / 'variants.json'
        record.write_text(
            json.dumps(result, sort_keys=True, separators=(',', ':')) + '\n')
        record.chmod(0o600)
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True)
    parser.add_argument('--target', required=True, choices=TARGETS)
    parser.add_argument('--candidate-assets', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--deadline', type=int, required=True)
    parser.add_argument('--build-deadline', type=int, required=True)
    parser.add_argument('--cargo', type=Path,
                        default=Path.home() / '.cargo/bin/cargo')
    options = parser.parse_args()
    cancellation_handlers()
    builder = None
    try:
        require(re.fullmatch(r'[0-9a-f]{40}', options.source), 'lifecycle_source_commit')
        require((platform.system(), platform.machine()) == TARGETS[options.target], 'lifecycle_host_target')
        require(0 < options.deadline - time.time() <= 5100, 'lifecycle_internal_deadline')
        require(30 < options.build_deadline - time.time() <= 2700
                and options.build_deadline <= options.deadline - 2100, 'lifecycle_build_deadline')
        builder = Build(options)
        builder.build()
        print(json.dumps({'schema': 1, 'ok': True, 'stage': 'complete'}))
    except (Refused, OSError, ValueError, KeyError, UnicodeError, subprocess.TimeoutExpired) as error:
        category = str(error) if isinstance(error, Refused) else 'lifecycle_build_refused'
        print(json.dumps({'schema': 1, 'ok': False,
                          'stage': builder.stage if builder else 'arguments', 'category': category}))
        parser.exit(1)


if __name__ == '__main__':
    main()
