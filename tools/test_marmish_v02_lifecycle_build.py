"""Offline builder contract tests; these do not claim compiled lifecycle proof."""
import argparse
import os
import signal
import sys
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

import marmish_v02_lifecycle_build as build


def sources():
    return dict(zip(build.SOURCE_FILES, (
        '[package]\nname = "marmish-native"\nversion = "0.2.0"\n\n[dependencies]\nkeep = "=1"\n',
        '[[package]]\nname = "marmish-native"\nversion = "0.2.0"\n\n[[package]]\nname = "keep"\nversion = "1"\nchecksum = "unchanged"\n',
        'pub(crate) const JOURNAL_FORMAT: i64 = 6;\n'
        'pub(crate) const JOURNAL_SUPPORTED_FORMATS: &[i64] = &[0, 3, 4, 5, 6];\n'
        '// remaining journal code unchanged\n')))


class PatchTests(unittest.TestCase):
    def test_exact_a_version_only_and_c_closed_format_patch(self):
        original = sources()
        a, number = build.patch_sources(original, 'a')
        self.assertEqual(number, 6)
        self.assertEqual(a[build.SOURCE_FILES[2]], original[build.SOURCE_FILES[2]])
        for name in build.SOURCE_FILES[:2]:
            self.assertEqual(a[name].replace('0.2.0-rc.1', '0.2.0'), original[name])
        c, _ = build.patch_sources(original, 'c')
        self.assertEqual(c[build.SOURCE_FILES[2]], original[build.SOURCE_FILES[2]].replace(
            'i64 = 6;', 'i64 = 4;').replace('&[0, 3, 4, 5, 6]', '&[0, 3, 4]'))
        self.assertEqual(original, sources())

    def test_unexpected_version_duplicate_or_missing_declaration_refused(self):
        for name in build.SOURCE_FILES:
            with self.subTest(name=name):
                for value in ('', sources()[name] * 2):
                    altered = sources()
                    altered[name] = value
                    with self.assertRaises(build.Refused):
                        build.patch_sources(altered, 'c')
        altered = sources()
        altered[build.SOURCE_FILES[1]] = altered[build.SOURCE_FILES[1]].replace('0.2.0', '0.2.1')
        with self.assertRaises(build.Refused):
            build.patch_sources(altered, 'a')

    def test_candidate_format_derived_not_hardcoded_and_must_be_newer(self):
        original = sources()
        original[build.SOURCE_FILES[2]] = original[build.SOURCE_FILES[2]].replace(
            'i64 = 6;', 'i64 = 7;').replace('4, 5, 6]', '4, 5, 6, 7]')
        _, number = build.patch_sources(original, 'c')
        self.assertEqual(number, 7)
        original[build.SOURCE_FILES[2]] = original[build.SOURCE_FILES[2]].replace('i64 = 7;', 'i64 = 4;')
        with self.assertRaises(build.Refused):
            build.patch_sources(original, 'c')

    def test_build_environment_excludes_credential_and_compiler_overrides(self):
        with patch.dict(os.environ, {'GH_TOKEN': 'test-only', 'CARGO_REGISTRY_TOKEN': 'test-only',
                                     'RUSTFLAGS': '--cfg not_allowed',
                                     'CARGO_TARGET_DIR': '/not-ours'}, clear=False):
            env = build.build_environment()
        self.assertTrue({'GH_TOKEN', 'CARGO_REGISTRY_TOKEN', 'RUSTFLAGS',
                         'CARGO_TARGET_DIR'}.isdisjoint(env))
        self.assertEqual(env['RUSTUP_TOOLCHAIN'], '1.97.1')


class FilesAndGitTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='marmish-lifecycle-builder-test-')
        self.root = Path(self.temp.name).resolve()

    def tearDown(self):
        self.temp.cleanup()

    def test_asset_inventory_rejects_missing_extra_links_and_detects_mutation(self):
        target = 'x86_64-unknown-linux-gnu'
        assets = self.root / 'assets'
        assets.mkdir()
        for name in build.FILES:
            (assets / f'{name}-0.2.0-{target}').write_bytes(b'offline-placeholder')
        manifest = assets / build.manifest_name('0.2.0', target)
        manifest.write_bytes(b'offline-placeholder')
        before = build.inventory(assets, '0.2.0', target)
        manifest.write_bytes(b'changed-placeholder')
        self.assertNotEqual(build.inventory(assets, '0.2.0', target), before)
        (assets / 'extra').touch()
        with self.assertRaises(build.Refused):
            build.inventory(assets, '0.2.0', target)
        (assets / 'extra').unlink()
        manifest.unlink()
        with self.assertRaises(build.Refused):
            build.inventory(assets, '0.2.0', target)
        outside = self.root / 'outside'
        outside.write_bytes(b'not-owned-input')
        manifest.symlink_to(outside)
        with self.assertRaises(build.Refused):
            build.inventory(assets, '0.2.0', target)
        manifest.unlink()
        os.link(outside, manifest)
        with self.assertRaises(build.Refused):
            build.inventory(assets, '0.2.0', target)

    def test_genuine_variant_commits_share_base_and_preserve_exact_patch_bytes(self):
        repo = self.root / 'repo'
        repo.mkdir()
        environment = dict(os.environ, GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull,
                           GIT_AUTHOR_NAME='fixture', GIT_COMMITTER_NAME='fixture',
                           GIT_AUTHOR_EMAIL='fixture@example.invalid',
                           GIT_COMMITTER_EMAIL='fixture@example.invalid')
        def git(*arguments):
            return subprocess.check_output(['git', '-c', 'core.hooksPath=' + os.devnull,
                                            '-C', str(repo), *arguments], env=environment,
                                           stderr=subprocess.DEVNULL).decode().strip()
        git('init', '--quiet')
        for name, text in sources().items():
            path = repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        (repo / 'other').write_text('base')
        git('add', '.')
        git('-c', 'commit.gpgsign=false', 'commit', '--quiet', '--no-gpg-sign', '-m', 'base')
        source = git('rev-parse', 'HEAD')
        options = argparse.Namespace(source=source, output=self.root / 'output',
            deadline=int(time.time()) + 5100, build_deadline=int(time.time()) + 2700, cargo=Path(shutil.which('cargo')))
        builder = build.Build(options)
        builder.env.update(environment)
        builder.clone = repo
        builder.original = sources()
        (builder.root / 'patches').mkdir()
        commits = []
        for variant in ('a', 'c'):
            commit, tree, digest = builder.commit_variant(variant)
            commits.append(commit)
            self.assertEqual(git('rev-parse', 'HEAD^'), source)
            self.assertEqual(git('rev-parse', 'HEAD^{tree}'), tree)
            raw = subprocess.check_output(['git', '-C', str(repo), 'diff', '--no-ext-diff',
                                            '--binary', source, commit])
            saved = (builder.root / 'patches' / (variant + '.patch')).read_bytes()
            self.assertEqual(raw, saved)
            self.assertEqual(build.sha(saved), digest)
        self.assertNotEqual(*commits)
        for unexpected in ('other', build.SOURCE_FILES[2]):
            git('checkout', '--detach', source)
            with (repo / unexpected).open('a') as stream:
                stream.write('\n// unexpected change\n')
            with self.assertRaisesRegex(build.Refused, 'variant_patch_scope'):
                builder.commit_variant('a')
            git('reset', '--hard', source)

    def fixture_assets(self, version='0.2.0-rc.2', journal=4):
        target = 'x86_64-unknown-linux-gnu'
        directory = self.root / 'assets'
        directory.mkdir()
        lines = ['marmish-manifest 1', 'version ' + version, 'target ' + target,
            'commit ' + 'a' * 40, 'signing none notarized=no', 'minimum glibc=2.39',
            f'state profile=1 journal={journal} engine=1 config=1 commands=1']
        for name in build.FILES:
            raw = ('offline fixture ' + name).encode()
            (directory / f'{name}-{version}-{target}').write_bytes(raw)
            lines.append(f'file {build.sha(raw)} {len(raw)} '
                + ('0444' if name == build.FILES[-1] else '0555') + ' ' + name)
        (directory / build.manifest_name(version, target)).write_text('\n'.join(lines + ['end', '']))
        return directory, target

    def test_relabelled_mixed_and_wrong_c_formats_refused(self):
        assets, target = self.fixture_assets()
        manifest = assets / build.manifest_name('0.2.0-rc.2', target)
        original = manifest.read_bytes()
        manifest.write_bytes(original.replace(b'version 0.2.0-rc.2', b'version 0.2.0'))
        with self.assertRaisesRegex(build.Refused, 'manifest_binding'):
            build.verify_assets(assets, '0.2.0-rc.2', 'a' * 40, target, self.root / 'bad-version')
        manifest.write_bytes(original)
        binary = assets / f'marmish-0.2.0-rc.2-{target}'
        raw = binary.read_bytes()
        binary.write_bytes(raw.replace(b'fixture', b'mixedxx'))
        with self.assertRaisesRegex(build.Refused, 'installed_file_digest'):
            build.verify_assets(assets, '0.2.0-rc.2', 'a' * 40, target, self.root / 'mixed')
        binary.write_bytes(raw)
        manifest.write_bytes(original.replace(b'journal=4', b'journal=6'))
        with self.assertRaisesRegex(build.Refused, 'variant_compiled_formats'):
            build.verify_assets(assets, '0.2.0-rc.2', 'a' * 40, target, self.root / 'format', [1, 4, 1, 1, 1])

    def test_candidate_mutation_during_verification_refused(self):
        assets, target = self.fixture_assets(version='0.2.0', journal=6)
        original = build.checks.check_manifest
        def mutate(*arguments):
            result = original(*arguments)
            (assets / f'marmish-0.2.0-{target}').write_bytes(b'changed')
            return result
        with patch.object(build.checks, 'check_manifest', side_effect=mutate):
            with self.assertRaisesRegex(build.Refused, 'candidate_assets_changed'):
                build.verify_assets(assets, '0.2.0', 'a' * 40, target, self.root / 'verify')

    def test_signalled_builder_reaps_separate_child_session(self):
        pid_file = self.root / 'child-pid'
        child_code = ('import os,time,signal;signal.signal(signal.SIGTERM,signal.SIG_IGN);open('
            + repr(str(pid_file)) + ',"w").write(str(os.getpid()));time.sleep(30)')
        parent_code = ('import marmish_v02_lifecycle_build as b; b.cancellation_handlers();'
            'b.execute(' + repr([sys.executable, '-c', child_code]) + ',{},timeout=20)')
        parent = subprocess.Popen([sys.executable, '-c', parent_code],
            env=dict(os.environ, PYTHONPATH=str(Path(build.__file__).parent)),
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        child_pid = None
        try:
            deadline = time.monotonic() + 5
            while not pid_file.exists() and parent.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(pid_file.exists())
            child_pid = int(pid_file.read_text())
            os.killpg(parent.pid, signal.SIGTERM)
            time.sleep(0.1)
            os.killpg(parent.pid, signal.SIGINT)
            self.assertNotEqual(parent.wait(timeout=8), 0)
            with self.assertRaises(ProcessLookupError):
                os.killpg(child_pid, 0)
        finally:
            if parent.poll() is None:
                os.killpg(parent.pid, signal.SIGKILL)
                parent.wait(timeout=3)
            if child_pid:
                try:
                    os.killpg(child_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_original_deadline_reserves_runtime_and_output_cannot_preexist(self):
        for remaining in (0, 2000):
            with self.assertRaises(build.Refused):
                build.Build(argparse.Namespace(deadline=int(time.time()) + remaining,
                    build_deadline=int(time.time()) + 2700, output=self.root / 'unused', cargo=Path(shutil.which('cargo'))))
        existing = self.root / 'existing'
        existing.mkdir()
        with self.assertRaises(FileExistsError):
            build.Build(argparse.Namespace(deadline=int(time.time()) + 5100,
                build_deadline=int(time.time()) + 2700, output=existing, cargo=Path(shutil.which('cargo'))))


if __name__ == '__main__':
    unittest.main()
