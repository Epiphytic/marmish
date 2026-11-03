"""Collect original dependency notices from the exact locked native graphs."""
from __future__ import annotations

import hashlib
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
import tomllib
import urllib.error
import urllib.parse
import urllib.request


LICENSE_NAME = re.compile(r'(?:licen[cs]e|copying|notice|copyright)(?:[._-].*)?', re.I)
REVIEWED_LICENSES = {
    'MIT', 'Apache-2.0', 'Unicode-3.0', 'Unicode-DFS-2016', 'BSD-2-Clause',
    'BSD-3-Clause', 'ISC', 'MPL-2.0', 'CC0-1.0', 'Unlicense', 'Zlib', 'BSL-1.0',
    'LLVM-exception', 'OpenSSL', 'CDLA-Permissive-2.0', '0BSD', 'MIT-0', 'WTFPL',
}
SOURCES = {'native': 'https://github.com/Epiphytic/marmish', 'marmish': 'https://github.com/Epiphytic/marmish',
           'mdk': 'https://github.com/liamhelmer-bel/mdk'}
NATIVE_NOTICES = {
    'libsqlite3-sys': ('sqlcipher/LICENSE',),
    'openssl-src': ('openssl/LICENSE.txt',),
    'secp256k1-sys': ('depend/secp256k1/COPYING',),
}
MAX_TEXT = 2 * 1024 * 1024
MAX_TOTAL = 128 * 1024 * 1024
ROOTS = {'native': {'marmish-native'}, 'marmish': {'marmish-engine', 'marmish-prototype', 'marmish-vault'},
         'mdk': {'wn-cli'}}
UPSTREAM_FILES = ('LICENSE', 'LICENCE', 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE.txt',
                  'LICENSE.md', 'COPYING', 'NOTICE', 'COPYRIGHT', 'LICENSES/MIT.txt', 'MPL-2.0.txt')
UPSTREAM_CACHE = {}
# Exact upstream release revisions for published crates missing cargo VCS metadata.
# Verification evidence is recorded under docs/validation/v0.2/native-license-sources-2026-11-03.json.
# The pre-existing bitcoin-io 0.1.4 override was verified against signed tag
# bitcoin-io-0.1.4 and io/Cargo.toml (version/license) at the pinned commit.
SOURCE_OVERRIDES = {
    ('bitcoin-io', '0.1.4'): ('rust-bitcoin/rust-bitcoin', '29a07abb2f1cf91ba7aa4820216ea091ee8b91e8'),
    ('bitcoin-consensus-encoding', '1.3.0'): ('rust-bitcoin/rust-bitcoin', '0cfc7908ff7c7c58e9c61c008c09cb2aeedb82cb'),
    ('bitcoin-internals', '0.6.0'): ('rust-bitcoin/rust-bitcoin', '3812750b6824eb0374504d7e2fddd8e8528a2cda'),
    ('bitcoin-internals', '0.7.0'): ('rust-bitcoin/rust-bitcoin', 'a53101246ec046340d0127d520192d00d39d261d'),
    ('bitcoin-io', '0.1.101'): ('rust-bitcoin/rust-bitcoin', 'a010f1e9fd7752ee5e7ca60a909d32a58e0d3297'),
    ('bitcoin_hashes', '0.14.101'): ('rust-bitcoin/rust-bitcoin', 'a010f1e9fd7752ee5e7ca60a909d32a58e0d3297'),
    ('bitcoin_hashes', '1.2.0'): ('rust-bitcoin/rust-bitcoin', '0f68fcb392512b5707529f864fb87882306e33d9'),
}
# Exact optional-license choice; preserve the original expression in the manifest.
LICENSE_CHOICES = {('priority-queue', '2.7.0'):
    ('LGPL-3.0-or-later OR MPL-2.0', 'MPL-2.0', 'MPL-2.0.txt')}
TOR_REPOSITORIES = ('https://gitlab.torproject.org/tpo/core/arti',
                    'https://gitlab.torproject.org/Diziet/rust-derive-deftly')
VOID_DECLARATION = 'Void 1.0.2 upstream supplies an MIT declaration and author attribution in README.md, but no full license file; the separate SPDX MIT template is canonical explanatory text, not an upstream copyright notice.'
VOID_NOTICES = (
    ('README.md', 'https://raw.githubusercontent.com/reem/rust-void/ab2f4dbb7c95c144ccba2fa8afd11e155aa91133/README.md',
     'f85783a6fcf9ecc19edabd710775a88430d9e886f46728bfd7d65cef55ff3e73'),
    ('canonical-license-template/MIT.txt', 'https://raw.githubusercontent.com/spdx/license-list-data/c4a7237ec8f4654e867546f9f409749300f1bf4c/text/MIT.txt',
     'b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5'))


def _production_packages(metadata, graph):
    packages = {package['id']: package for package in metadata['packages']}
    nodes = {node['id']: node for node in metadata['resolve']['nodes']}
    pending = [key for key, package in packages.items() if package['name'] in ROOTS[graph]]
    if {packages[key]['name'] for key in pending} != ROOTS[graph]:
        raise RuntimeError('missing_production_license_root')
    selected = set()
    while pending:
        key = pending.pop()
        if key not in selected:
            selected.add(key)
            pending.extend(dep['pkg'] for dep in nodes[key]['deps']
                if any(kind['kind'] != 'dev' for kind in dep['dep_kinds']))
    return [packages[key] for key in sorted(selected)]


class _RawRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, new_url):
        parsed = urllib.parse.urlsplit(new_url)
        if parsed.scheme != 'https' or not (parsed.netloc == 'raw.githubusercontent.com' or
                (parsed.netloc == 'gitlab.torproject.org' and
                 any(parsed.path.startswith(urllib.parse.urlsplit(repo).path + '/-/raw/')
                     for repo in TOR_REPOSITORIES))):
            raise RuntimeError('unsafe_license_redirect')
        return super().redirect_request(request, fp, code, message, headers, new_url)


def _download_notice(url):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), _RawRedirect())
    try:
        with opener.open(url, timeout=30) as response:
            raw = response.read(MAX_TEXT + 1)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise RuntimeError('license_source_download_failed') from None
    if not 0 < len(raw) <= MAX_TEXT or b'\0' in raw:
        raise RuntimeError('invalid_license_text')
    raw.decode('utf-8')
    return raw


def _upstream_identity(package):
    override = SOURCE_OVERRIDES.get((package['name'], package['version']))
    if override:
        return override
    repository = (package.get('repository') or '').rstrip('/').removesuffix('.git')
    if (repository not in TOR_REPOSITORIES and
            not re.fullmatch(r'https://github.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository)):
        raise RuntimeError('unreviewed_license_source:' + package['name'])
    vcs = json.loads(_read(Path(package['manifest_path']).parent / '.cargo_vcs_info.json'))
    revision = vcs.get('git', {}).get('sha1')
    if not isinstance(revision, str) or not re.fullmatch('[0-9a-f]{40}', revision) or vcs['git'].get('dirty'):
        raise RuntimeError('invalid_license_source_revision')
    return repository.removeprefix('https://github.com/'), revision


def _upstream_notices(package):
    if (package['name'], package['version']) == ('void', '1.0.2'):
        if (package.get('source') != 'registry+https://github.com/rust-lang/crates.io-index'
                or package.get('license') != 'MIT'
                or (package.get('repository') or '').rstrip('/').removesuffix('.git') != 'https://github.com/reem/rust-void'):
            raise RuntimeError('void_declaration_source_mismatch')
        notices = []
        for name, url, expected in VOID_NOTICES:
            raw = _download_notice(url)
            if raw is None or hashlib.sha256(raw).hexdigest() != expected:
                raise RuntimeError('void_declaration_digest_mismatch')
            notices.append((name, raw, url))
        return notices
    identity = _upstream_identity(package)
    if identity not in UPSTREAM_CACHE:
        if identity[0] in TOR_REPOSITORIES:
            base = identity[0] + '/-/raw/' + identity[1] + '/'
        else:
            base = 'https://raw.githubusercontent.com/' + identity[0] + '/' + identity[1] + '/'
        with ThreadPoolExecutor(max_workers=4) as pool:
            raw = list(pool.map(_download_notice, [base + name for name in UPSTREAM_FILES]))
        UPSTREAM_CACHE[identity] = [(name, data, base + name)
            for name, data in zip(UPSTREAM_FILES, raw) if data is not None]
    notices = UPSTREAM_CACHE[identity]
    if not notices:
        raise RuntimeError('missing_license_text:' + package['name'])
    return notices


def _environment():
    from marmish_ci import build_environment
    environment = build_environment()
    if 'RUSTUP_HOME' in os.environ:
        environment['RUSTUP_HOME'] = os.environ['RUSTUP_HOME']
    return environment


def _target():
    return {('Linux', 'x86_64'): 'x86_64-unknown-linux-gnu',
            ('Linux', 'aarch64'): 'aarch64-unknown-linux-gnu',
            ('Darwin', 'arm64'): 'aarch64-apple-darwin'}[(platform.system(), platform.machine())]


def _metadata(root):
    selector = [] if os.environ.get('MARMISH_NATIVE_TOOLCHAIN') == '1' else ['+1.97.1']
    raw = subprocess.check_output(['cargo', *selector, 'metadata', '--locked',
        '--filter-platform', _target(), '--format-version', '1'], cwd=root,
        env=_environment(), timeout=600)
    if len(raw) > 32 * 1024 * 1024:
        raise RuntimeError('license_metadata_oversized')
    return json.loads(raw)


def _read(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= MAX_TEXT:
            raise RuntimeError('invalid_license_text')
        raw = stream.read(MAX_TEXT + 1)
    if len(raw) > MAX_TEXT or b'\0' in raw:
        raise RuntimeError('invalid_license_text')
    raw.decode('utf-8')
    return raw


def _reviewed_license(expression):
    if not isinstance(expression, str) or not expression:
        raise RuntimeError('missing_license_metadata')
    identifiers = re.findall(r'[A-Za-z0-9][A-Za-z0-9.+-]*', expression)
    if (re.sub(r'[A-Za-z0-9.+\-() /]', '', expression) or
            any(word not in REVIEWED_LICENSES | {'AND', 'OR', 'WITH'} for word in identifiers)):
        raise RuntimeError('unreviewed_license_expression:' + expression)


def _notice_files(directory):
    count = 0
    for current, directories, filenames in os.walk(directory, followlinks=False):
        directories[:] = sorted(name for name in directories
            if name not in ('.git', 'target', '__pycache__')
            and not (Path(current) / name).is_symlink())
        count += len(directories) + len(filenames)
        if count > 100000:
            raise RuntimeError('license_tree_oversized')
        for name in sorted(filenames):
            if LICENSE_NAME.fullmatch(name):
                yield Path(current) / name


def _package_notices(package, root):
    root = root.resolve()
    directory = Path(package['manifest_path']).parent.resolve()
    paths = {path: str(path.relative_to(directory)) for path in _notice_files(directory)}
    if package.get('license_file'):
        declared = directory / package['license_file']
        if not declared.resolve().is_relative_to(directory):
            raise RuntimeError('external_declared_license')
        paths[declared] = str(declared.relative_to(directory))
    if package.get('source') is None:
        if not directory.is_relative_to(root):
            raise RuntimeError('external_local_dependency')
        ancestor = root
    elif package['source'].startswith('git+'):
        ancestor = Path(subprocess.check_output(['git', '-C', str(directory),
            'rev-parse', '--show-toplevel'], text=True, env=_environment()).strip()).resolve()
    else:
        ancestor = directory
    if ancestor != directory:
        for path in ancestor.iterdir():
            if LICENSE_NAME.fullmatch(path.name) and path.is_file():
                paths[path] = 'source-root/' + path.name
    for required in NATIVE_NOTICES.get(package['name'], ()):
        if directory / required not in paths:
            raise RuntimeError('missing_native_license:' + package['name'] + ':' + required)
    result = []
    for path, original in sorted(paths.items(), key=lambda item: item[1]):
        resolved = path.resolve(strict=True)
        if not resolved.is_relative_to(ancestor):
            raise RuntimeError('external_license_text')
        result.append((resolved, original))
    return result


def _source(package, graph_source, lock):
    source = package.get('source')
    if source is None:
        return dict(graph_source)
    if source.startswith('registry+'):
        checksum = lock.get((package['name'], package['version'], source), {}).get('checksum')
        if not isinstance(checksum, str) or not re.fullmatch('[0-9a-f]{64}', checksum):
            raise RuntimeError('missing_registry_checksum')
        return {'url': 'https://static.crates.io/crates/' + package['name'] + '/' +
                package['name'] + '-' + package['version'] + '.crate', 'checksum': checksum}
    if source.startswith('git+') and '#' in source:
        remote, revision = source[4:].rsplit('#', 1)
        if not re.fullmatch('[0-9a-f]{40}', revision) or not remote.startswith('https://'):
            raise RuntimeError('invalid_git_license_source')
        return {'url': remote.split('?', 1)[0], 'revision': revision}
    raise RuntimeError('unreviewed_dependency_source')


def _package_record(package, root, graph_source, lock, texts):
    expression = package.get('license')
    inherited = (package['name'] == 'marmot-markdown' and package.get('source') is None
        and graph_source == {'url': SOURCES['mdk'], 'revision':
                             '37c248a11239e108afc9bd4ee62b8f60542012e3'})
    if not expression and inherited:
        expression = 'MIT'
    choice = LICENSE_CHOICES.get((package['name'], package['version']))
    override = 'MIT inherited from pinned MDK root LICENSE' if not package.get('license') else None
    if choice:
        if expression != choice[0]:
            raise RuntimeError('license_choice_expression_mismatch')
        override = 'Selected ' + choice[1] + ' from declared SPDX expression: ' + expression
        expression = choice[1]
    _reviewed_license(expression)
    notices = []
    for path, original in _package_notices(package, root):
        raw = _read(path)
        digest = hashlib.sha256(raw).hexdigest()
        texts[digest] = raw
        notices.append({'original_path': original, 'sha256': digest,
                        'text': 'texts/' + digest + '.txt'})
    if not notices:
        for name, raw, url in _upstream_notices(package):
            digest = hashlib.sha256(raw).hexdigest()
            texts[digest] = raw
            original = name if name.startswith('canonical-license-template/') else 'upstream-root/' + name
            notices.append({'original_path': original, 'sha256': digest,
                            'text': 'texts/' + digest + '.txt', 'source_url': url})
        if (package['name'], package['version']) == ('void', '1.0.2'):
            override = VOID_DECLARATION
    if choice and not any(n['original_path'] == choice[2] or
            n['original_path'] == 'upstream-root/' + choice[2] for n in notices):
        raise RuntimeError('missing_selected_license_text')
    return {'name': package['name'], 'version': package['version'],
            'license': expression, 'source': _source(package, graph_source, lock),
            'notices': notices, 'license_override': override}


def _source_notice(graph_source, packages, native=False):
    lines = ['Original dependency license texts accompany this file.',
        'Each manifest source URL identifies the exact source form used.',
        'Registry archives retain their Cargo.lock SHA256 checksums.',
        'Git sources identify full immutable commits.',
        'MPL-2.0 source remains available under MPL-2.0 at the listed source URL.',
        'No MPL dependency source files were modified by Marmish.',
        'Application source: ' + graph_source['url'] + ' commit ' + graph_source['revision'], '']
    if not native:
        lines.insert(6, 'Marmish MDK patches are distributed in patches/mdk/ of the runtime bundle.')
    for package in packages:
        source = package['source']
        lines.append(package['name'] + ' ' + package['version'] + ' (' + package['license'] + ')')
        lines.append('  ' + source['url'] + ' ' + source.get('revision', source.get('checksum', '')))
    return '\n'.join(lines) + '\n'


def collect(root: Path, output: Path, graph_name: str) -> None:
    """Write original notices, source references and a reproducible locked graph manifest."""
    if graph_name not in SOURCES:
        raise RuntimeError('invalid_license_graph')
    root = root.resolve()
    destination = output / graph_name
    if destination.exists() or destination.is_symlink():
        raise RuntimeError('license_destination_exists')
    metadata = _metadata(root)
    lock_raw = (root / 'Cargo.lock').read_bytes()
    lock = {(p['name'], p['version'], p.get('source')): p
            for p in tomllib.loads(lock_raw.decode())['package']}
    revision = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'],
        text=True, env=_environment()).strip()
    if not re.fullmatch('[0-9a-f]{40}', revision):
        raise RuntimeError('invalid_graph_revision')
    source = {'url': SOURCES[graph_name], 'revision': revision}
    texts = {}
    packages = [_package_record(package, root, source, lock, texts)
                for package in sorted(_production_packages(metadata, graph_name),
                    key=lambda p: (p['name'], p['version'], p.get('source') or ''))]
    if sum(map(len, texts.values())) > MAX_TOTAL:
        raise RuntimeError('license_collection_oversized')
    (destination / 'texts').mkdir(parents=True)
    for digest, raw in sorted(texts.items()):
        (destination / 'texts' / (digest + '.txt')).write_bytes(raw)
    manifest = {'format': 1, 'graph': graph_name, 'target': _target(), 'source': source,
        'cargo_lock_sha256': hashlib.sha256(lock_raw).hexdigest(), 'packages': packages}
    (destination / 'manifest.json').write_text(json.dumps(manifest, sort_keys=True, indent=2) + '\n')
    (destination / 'SOURCES.txt').write_text(_source_notice(source, packages, graph_name == 'native'))


def _validate_package(package, directory):
    if set(package) != {'name', 'version', 'license', 'source', 'notices', 'license_override'}:
        raise RuntimeError('invalid_license_package')
    if not all(isinstance(package[key], str) and re.fullmatch('[A-Za-z0-9_.+-]+', package[key])
               for key in ('name', 'version')):
        raise RuntimeError('invalid_license_package')
    _reviewed_license(package['license'])
    source = package['source']
    if (not isinstance(source, dict) or not isinstance(source.get('url'), str)
            or not source['url'].startswith('https://') or len(source) != 2):
        raise RuntimeError('invalid_license_source')
    key, pattern = ('revision', '[0-9a-f]{40}') if 'revision' in source else ('checksum', '[0-9a-f]{64}')
    if not isinstance(source.get(key), str) or not re.fullmatch(pattern, source[key]):
        raise RuntimeError('invalid_license_source')
    if not isinstance(package['notices'], list) or not package['notices']:
        raise RuntimeError('missing_license_text')
    referenced = set()
    for notice in package['notices']:
        digest = notice.get('sha256')
        if (not isinstance(digest, str) or not re.fullmatch('[0-9a-f]{64}', digest)
                or notice.get('text') != 'texts/' + digest + '.txt'
                or not isinstance(notice.get('original_path'), str)):
            raise RuntimeError('invalid_license_notice')
        if hashlib.sha256(_read(directory / notice['text'])).hexdigest() != digest:
            raise RuntimeError('license_digest_mismatch')
        referenced.add(notice['text'])
    return referenced


def validate_graph(directory: Path) -> None:
    """Validate a previously collected graph without Cargo, network or provider access."""
    manifest_path = directory / 'manifest.json'
    if manifest_path.is_symlink() or manifest_path.stat().st_size > 8 * 1024 * 1024:
        raise RuntimeError('invalid_license_manifest')
    manifest = json.loads(manifest_path.read_bytes())
    if (set(manifest) != {'format', 'graph', 'target', 'source', 'cargo_lock_sha256', 'packages'}
            or type(manifest['format']) is not int or manifest['format'] != 1
            or manifest['graph'] != directory.name or directory.name not in ROOTS
            or manifest['target'] not in ('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
                                         'aarch64-apple-darwin')
            or not re.fullmatch('[0-9a-f]{64}', manifest['cargo_lock_sha256'])
            or not isinstance(manifest['packages'], list) or not 1 <= len(manifest['packages']) <= 2000):
        raise RuntimeError('invalid_license_manifest')
    source = manifest['source']
    if (set(source) != {'url', 'revision'} or source['url'] != SOURCES[directory.name]
            or not re.fullmatch('[0-9a-f]{40}', source['revision'])):
        raise RuntimeError('invalid_license_graph_source')
    if not ROOTS[directory.name] <= {package.get('name') for package in manifest['packages']}:
        raise RuntimeError('missing_production_license_root')
    expected = {'manifest.json', 'SOURCES.txt'}
    for package in manifest['packages']:
        expected |= _validate_package(package, directory)
    actual = set()
    for path in directory.rglob('*'):
        if path.is_symlink() or not (path.is_dir() or path.is_file()):
            raise RuntimeError('unsafe_license_output')
        if path.is_file():
            actual.add(path.relative_to(directory).as_posix())
    if actual != expected:
        raise RuntimeError('unexpected_license_output')
    if (directory / 'SOURCES.txt').read_text() != _source_notice(source, manifest['packages'], directory.name == 'native'):
        raise RuntimeError('invalid_license_source_notice')


def validate(output: Path) -> None:
    # Preserve the legacy dual-graph validation entrypoint; native tooling uses
    # validate_graph explicitly for its separate single production graph.
    for graph in ('marmish', 'mdk'):
        validate_graph(output / graph)
