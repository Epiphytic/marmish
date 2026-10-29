"""Host-only named native keys and durable profile bindings.

Secrets travel only through private helper stdio and in-memory return values.
Direct Python use cannot promise erasure of immutable serialization copies.
"""
import base64
from dataclasses import dataclass, field
import fcntl
import os
from pathlib import Path
import re
import selectors
import secrets
import signal
import shutil
import stat
import subprocess
import sys
import unicodedata

from mdk_smoke import SmokeError, public_key, require
from marmish_platform import continuous_time, execution_marker
from marmish_protocol import decode, encode, private_directory, save_json

CATEGORIES = {'unavailable', 'uninitialized', 'locked_or_denied', 'missing',
              'exists', 'ambiguous', 'malformed', 'backend_failure'}


def _id(value):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value), 'invalid_key_binding')
    return value


def _reference(value, purpose):
    require(isinstance(value, dict) and set(value) == {'version', 'purpose', 'id'}
            and type(value['version']) is int and value['version'] == 1
            and value['purpose'] == purpose, 'invalid_key_binding')
    _id(value['id'])
    return dict(value)


def _label(value):
    try:
        size = len(value.encode('utf-8')) if isinstance(value, str) else 0
    except UnicodeError:
        raise SmokeError('invalid_key_name') from None
    require(isinstance(value, str) and 1 <= size <= 128
            and not any(unicodedata.category(c) == 'Cc' or '\u2028' <= c <= '\u202e'
                        or '\u2066' <= c <= '\u2069' for c in value), 'invalid_key_name')
    return value


def _trusted_path(path, *, executable=False):
    path = Path(path)
    require(path.is_absolute() and '..' not in path.parts, 'unsafe_key_path')
    for component in [*reversed(path.parents), path]:
        info = component.lstat()
        require(not stat.S_ISLNK(info.st_mode) and info.st_uid in (0, os.geteuid()), 'unsafe_key_path')
        sticky_root = stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and info.st_mode & stat.S_ISVTX
        require(not info.st_mode & 0o022 or sticky_root, 'unsafe_key_path')
    info = path.lstat()
    if executable:
        require(stat.S_ISREG(info.st_mode) and not info.st_mode & 0o6000
                and os.access(path, os.X_OK), 'unsafe_key_helper')
    return path


def protected_read(path, max_size=1048576):
    """Read an owned private regular file without following final symlinks."""
    path = _trusted_path(Path(path).absolute())
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
                and info.st_mode & 0o077 == 0 and info.st_nlink == 1
                and info.st_size <= max_size, 'unsafe_key_file')
        data = os.read(fd, max_size + 1)
        require(len(data) <= max_size, 'key_file_limit')
        return data
    finally:
        os.close(fd)


def protected_unlink(path, expected_bytes):
    path = Path(path)
    require(protected_read(path, len(expected_bytes)) == expected_bytes, 'legacy_key_mismatch')
    path.unlink()
    fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def _read(path, default=None):
    if not path.exists() and not path.is_symlink():
        return default
    return decode(protected_read(path), 1048576)


def _secret(value):
    require(isinstance(value, bytes) and len(value) == 32, 'invalid_key_secret')
    return value


class NativeHelper:
    def __init__(self, path, *, timeout=12):
        self.path = _trusted_path(Path(path).absolute(), executable=True)
        require(0 < timeout <= 15, 'invalid_helper_timeout')
        self.timeout = timeout

    def call(self, action, **fields):
        request = encode({'version': 1, 'action': action, **fields})
        require(len(request) <= 8192, 'key_helper_request_limit')
        env = {'PATH': '/usr/bin:/bin', **execution_marker()}
        for name in ('HOME', 'XDG_RUNTIME_DIR', 'DBUS_SESSION_BUS_ADDRESS'):
            if name in os.environ:
                env[name] = os.environ[name]
        _trusted_path(self.path, executable=True)
        try:
            process = subprocess.Popen([str(self.path)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, env=env, close_fds=True, start_new_session=True)
        except OSError:
            raise SmokeError('key_helper_start_failed') from None
        selector = selectors.DefaultSelector()
        output, diagnostics, offset = bytearray(), 0, 0
        deadline = continuous_time() + self.timeout
        reaped = False
        try:
            for stream, event in ((process.stdin, selectors.EVENT_WRITE),
                                  (process.stdout, selectors.EVENT_READ),
                                  (process.stderr, selectors.EVENT_READ)):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, event)
            while selector.get_map():
                remaining = deadline - continuous_time()
                require(remaining > 0, 'key_helper_timeout')
                for key, _ in selector.select(min(remaining, .2)):
                    stream = key.fileobj
                    if stream is process.stdin:
                        try:
                            offset += os.write(stream.fileno(), request[offset:])
                        except BrokenPipeError:
                            offset = len(request)
                        if offset == len(request):
                            selector.unregister(stream)
                            stream.close()
                    else:
                        data = os.read(stream.fileno(), 4096)
                        if not data:
                            selector.unregister(stream)
                            stream.close()
                        elif stream is process.stdout:
                            output.extend(data)
                            require(len(output) <= 131072, 'key_helper_response_limit')
                        else:
                            diagnostics += len(data)
                            require(diagnostics <= 8192, 'key_helper_response_limit')
            require(continuous_time() < deadline, 'key_helper_timeout')
            code = process.wait(timeout=max(.001, deadline - continuous_time()))
            reaped = True
            response = decode(bytes(output), 131072)
            require(set(response) == ({'version', 'ok', 'result'} if response.get('ok') is True
                                     else {'version', 'ok', 'error'})
                    and type(response.get('version')) is int and response['version'] == 1
                    and type(response.get('ok')) is bool, 'key_helper_response')
            if not response['ok']:
                require(isinstance(response['error'], str) and response['error'] in CATEGORIES and code == 1, 'key_helper_response')
                raise SmokeError(response['error'])
            require(code == 0 and isinstance(response['result'], dict), 'key_helper_response')
            return response['result']
        except subprocess.TimeoutExpired:
            raise SmokeError('key_helper_timeout') from None
        finally:
            selector.close()
            if not reaped:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            process.wait()
            for stream in (process.stdin, process.stdout, process.stderr):
                stream.close()
            output[:] = b'\0' * len(output)

    def probe(self):
        require(self.call('probe') == {}, 'key_helper_response')

    def derive_nostr(self, secret):
        value = self.call('derive_nostr', secret=base64.b64encode(_secret(secret)).decode('ascii'))
        require(set(value) == {'public_key'}, 'key_helper_response')
        return public_key(value['public_key'])

    def create(self, reference, label, secret):
        require(self.call('create', reference=reference, label=_label(label),
                          secret=base64.b64encode(_secret(secret)).decode('ascii')) == {}, 'key_helper_response')

    def load(self, reference):
        result = self.call('load', reference=reference)
        require(set(result) == {'secret'} and isinstance(result['secret'], str), 'key_helper_response')
        try:
            raw = base64.b64decode(result['secret'], validate=True)
        except (ValueError, TypeError):
            raise SmokeError('key_helper_response') from None
        require(base64.b64encode(raw).decode('ascii') == result['secret'], 'key_helper_response')
        return _secret(raw)


@dataclass(frozen=True)
class NostrBinding:
    backend: str
    profile_id: str
    reference: dict | None
    account: str | None
    secret: bytes | None = field(default=None, repr=False)

    def public(self):
        return {'backend': self.backend, 'profile_id': self.profile_id,
                'reference': self.reference, 'account': self.account}


@dataclass(frozen=True)
class MasterBinding:
    backend: str
    profile_id: str
    root_id: str | None
    reference: dict | None
    secret: bytes | None = field(default=None, repr=False)

    def public(self):
        return {'backend': self.backend, 'profile_id': self.profile_id,
                'root_id': self.root_id, 'reference': self.reference}


def _lock(path):
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    info = os.fstat(fd)
    try:
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
                and info.st_mode & 0o077 == 0 and info.st_nlink == 1, 'unsafe_key_lock')
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(fd)
        raise SmokeError('key_storage_busy') from None
    except BaseException:
        os.close(fd)
        raise
    return fd


class KeySession:
    """Profile→catalog lock order; commit releases the catalog before daemon use."""
    def __init__(self, root, *, storage='auto', key_name=None, helper=None, catalog=None):
        require(storage in ('auto', 'native', 'files'), 'invalid_key_storage')
        self.root = private_directory(root)
        _trusted_path(self.root)
        self.storage, self.name = storage, _label(key_name) if key_name is not None else None
        self.helper = helper
        self.catalog_path = Path(catalog) if catalog else self.default_catalog()
        self.profile_lock = self.catalog_lock = None
        self.nostr = self.master = None
        self.pending = None
        self.migration = False
        self.created_now = set()

    @staticmethod
    def default_catalog():
        if sys.platform == 'darwin':
            return Path.home() / 'Library/Application Support/Marmish/keys.json'
        return Path(os.environ.get('XDG_CONFIG_HOME', str(Path.home() / '.config'))) / 'marmish/keys.json'

    def __enter__(self):
        self.profile_lock = _lock(self.root / '.keys.lock')
        try:
            self.value = _read(self.root / 'profile.json', {})
            require(isinstance(self.value, dict), 'invalid_key_profile')
            self.pending = _read(self.root / 'keys.pending.json')
            self.selection = _read(self.root / 'keys.mode.json')
            if self.selection is not None:
                require(isinstance(self.selection, dict)
                        and set(self.selection) == {'version', 'backend', 'profile_id', 'profile'}
                        and type(self.selection['version']) is int and self.selection['version'] == 1
                        and self.selection['backend'] == 'files', 'invalid_key_selection')
                _id(self.selection['profile_id'])
            keys = self.value.get('keys')
            if keys is not None:
                require(isinstance(keys, dict) and set(keys) == {'version', 'backend', 'profile_id', 'nostr', 'master'}
                        and type(keys['version']) is int and keys['version'] == 1
                        and keys['backend'] in ('files', 'native'), 'invalid_key_binding')
            require(self.pending is None or isinstance(self.pending, dict) and 'profile_id' in self.pending, 'invalid_key_journal')
            self.profile_id = _id(keys['profile_id']) if keys is not None else (
                _id(self.pending['profile_id']) if self.pending else (
                    self.selection['profile_id'] if self.selection else secrets.token_hex(32)))
            self.backend = keys['backend'] if keys is not None else ('files' if self.selection else None)
            self.old_keys = keys
            if self.selection:
                require(self.selection['profile_id'] == self.profile_id
                        and self.selection['profile'] == self._context(), 'invalid_key_selection')
                require(self.backend != 'native' or self.pending is not None
                        and self.pending.get('migration') is True, 'mixed_key_storage')
            if keys is not None:
                self._binding(keys['nostr'], 'nostr')
                require(keys['nostr']['backend'] == keys['backend'], 'invalid_key_binding')
                if keys['master'] is not None:
                    self._binding(keys['master'], 'master')
                    require(keys['master']['backend'] == keys['backend'], 'invalid_key_binding')
            if self.pending:
                self._validate_pending()
                require(not self.selection or self.pending['migration'], 'mixed_key_storage')
                require(keys is None or keys['backend'] != 'files' or self.pending['migration'], 'mixed_key_storage')
                self.migration = self.pending['migration']
                if keys is not None and keys['backend'] == 'native':
                    require(keys['nostr'] == self.pending['nostr']
                            and (keys['master'] is None or keys['master'] == self.pending['master']),
                            'invalid_key_journal')
                exact = (keys is not None and keys['backend'] == 'native'
                         and keys['nostr'] == self.pending['nostr']
                         and keys['master'] == self.pending['master'])
                if self.pending['state'] in ('COMMITTED', 'CLEANED'):
                    require(exact, 'invalid_key_journal')
                elif exact:
                    require(self.pending['state'] == 'NATIVE_CREATED', 'invalid_key_journal')
                    self.pending['state'] = 'COMMITTED'
                    self._journal()
            return self
        except BaseException:
            self.__exit__()
            raise

    def __exit__(self, *_args):
        for name in ('catalog_lock', 'profile_lock'):
            fd = getattr(self, name)
            if fd is not None:
                os.close(fd)
                setattr(self, name, None)

    def _context(self):
        info = self.root.stat()
        return {'id': self.profile_id, 'path': str(self.root), 'device': info.st_dev, 'inode': info.st_ino}

    def _validate_pending(self):
        p = self.pending
        require(isinstance(p, dict) and set(p) == {'version', 'profile_id', 'profile', 'state', 'migration', 'nostr', 'master', 'verified'}
                and type(p['version']) is int and p['version'] == 1
                and p['profile_id'] == self.profile_id and p['profile'] == self._context()
                and p['state'] in ('PENDING', 'NATIVE_CREATED', 'COMMITTED', 'CLEANED')
                and type(p['migration']) is bool and isinstance(p['verified'], list)
                and all(isinstance(v, str) for v in p['verified'])
                and len(set(p['verified'])) == len(p['verified'])
                and set(p['verified']) <= {'nostr', 'master'}, 'invalid_key_journal')
        for kind in ('nostr', 'master'):
            if p[kind] is not None:
                self._binding(p[kind], kind)
                require(p[kind]['backend'] == 'native', 'invalid_key_journal')
        require(p['nostr'] is not None, 'invalid_key_journal')
        expected = {'nostr', 'master'} if p['master'] else {'nostr'}
        if p['state'] != 'PENDING':
            require(set(p['verified']) == expected, 'invalid_key_journal')

    def _binding(self, value, kind):
        fields = {'backend', 'profile_id', 'reference', 'account' if kind == 'nostr' else 'root_id'}
        require(isinstance(value, dict) and set(value) == fields
                and value['backend'] in ('files', 'native') and value['profile_id'] == self.profile_id,
                'invalid_key_binding')
        if value['backend'] == 'native':
            _reference(value['reference'], 'nostr' if kind == 'nostr' else 'sqlcipher')
            public_key(value['account']) if kind == 'nostr' else _id(value['root_id'])
        else:
            require(value['reference'] is None and (kind == 'nostr' or value['root_id'] is None), 'invalid_key_binding')
        return value

    def _helper(self):
        if self.helper is None:
            sibling = Path(sys.argv[0]).absolute().parent / 'marmish-keychain'
            found = sibling if sibling.is_file() else shutil.which('marmish-keychain')
            require(found is not None, 'key_helper_required')
            self.helper = Path(found).absolute()
        if isinstance(self.helper, (str, Path)):
            self.helper = NativeHelper(self.helper)
        return self.helper

    def _catalog(self):
        if self.catalog_lock is None:
            parent = private_directory(self.catalog_path.absolute().parent)
            _trusted_path(parent)
            self.catalog_path = parent / self.catalog_path.name
            self.catalog_lock = _lock(parent / '.keys.catalog.lock')
        catalog = _read(self.catalog_path, {'version': 1, 'entries': []})
        require(isinstance(catalog, dict) and set(catalog) == {'version', 'entries'} and type(catalog['version']) is int
                and catalog['version'] == 1 and isinstance(catalog['entries'], list)
                and len(catalog['entries']) <= 128, 'invalid_key_catalog')
        names, refs, profiles = set(), set(), set()
        for entry in catalog['entries']:
            require(isinstance(entry, dict) and set(entry) == {'name', 'profile', 'reference', 'account'}, 'invalid_key_catalog')
            _label(entry['name'])
            _reference(entry['reference'], 'nostr')
            public_key(entry['account'])
            context = entry['profile']
            require(isinstance(context, dict) and set(context) == {'id', 'path', 'device', 'inode'}
                    and isinstance(context['path'], str) and Path(context['path']).is_absolute()
                    and type(context['device']) is int and context['device'] >= 0
                    and type(context['inode']) is int and context['inode'] > 0, 'invalid_key_catalog')
            _id(context['id'])
            require(entry['name'] not in names and entry['reference']['id'] not in refs
                    and context['id'] not in profiles,
                    'invalid_key_catalog')
            names.add(entry['name'])
            refs.add(entry['reference']['id'])
            profiles.add(context['id'])
        return catalog

    def lock_catalog(self):
        """Acquire catalog before caller takes MDK-home and engine locks."""
        self._catalog()

    def _reserve(self, binding):
        catalog = self._catalog()
        found = [e for e in catalog['entries'] if e['reference'] == binding['reference']]
        named = [e for e in catalog['entries'] if self.name is not None and e['name'] == self.name]
        if found or named:
            require(len(found) == 1 and (not named or found == named)
                    and found[0]['profile'] == self._context() and found[0]['account'] == binding['account'],
                    'key_name_bound_elsewhere')
            if self.name is not None:
                require(found[0]['name'] == self.name, 'key_name_mismatch')
            self.name = found[0]['name']
            return
        require(not self.old_keys or self.old_keys['backend'] != 'native', 'key_catalog_missing')
        require(len(catalog['entries']) < 128, 'key_catalog_limit')
        self.name = self.name or 'marmish-' + self.profile_id[:16]
        require(not any(e['name'] == self.name for e in catalog['entries']), 'key_name_bound_elsewhere')
        catalog['entries'].append({'name': self.name, 'profile': self._context(),
                                   'reference': binding['reference'], 'account': binding['account']})
        save_json(self.catalog_path, catalog)

    def _journal(self):
        save_json(self.root / 'keys.pending.json', self.pending)

    def _candidate(self, kind, account=None):
        if kind == 'nostr':
            return {'backend': 'native', 'profile_id': self.profile_id,
                    'reference': {'version': 1, 'purpose': 'nostr', 'id': secrets.token_hex(32)}, 'account': account}
        return {'backend': 'native', 'profile_id': self.profile_id, 'root_id': secrets.token_hex(32),
                'reference': {'version': 1, 'purpose': 'sqlcipher', 'id': secrets.token_hex(32)}}

    def _new_pending(self, nostr, *, migration=False, master=None):
        self.pending = {'version': 1, 'profile_id': self.profile_id, 'profile': self._context(),
                        'state': 'PENDING', 'migration': migration, 'nostr': nostr, 'master': master, 'verified': []}
        self._journal()

    def prepare_nostr(self, *, existing_account=None, pristine=False, migration_secret=None):
        existing_account = public_key(existing_account) if existing_account is not None else None
        if self.backend is None:
            if self.pending:
                self.backend = 'native'
            elif self.value or not pristine:
                require(self.storage != 'native', 'explicit_key_migration_required')
                self.backend = 'files'
            elif self.storage == 'files':
                self.backend = 'files'
            else:
                try:
                    self._helper().probe()
                    self.backend = 'native'
                except SmokeError as error:
                    if self.storage != 'auto' or str(error) != 'unavailable':
                        raise
                    require(self.name is None, 'named_key_requires_native')
                    self.backend = 'files'
                    print('marmish: native key storage unavailable; using files', file=sys.stderr)
        require(self.migration or self.backend != 'files' or self.storage != 'native',
                'explicit_key_migration_required')
        require(self.migration or self.storage == 'auto' or self.storage == self.backend, 'key_storage_conflict')
        if self.backend == 'files':
            require(self.name is None and migration_secret is None, 'named_key_requires_native')
            self.nostr = NostrBinding('files', self.profile_id, None, existing_account)
            if self.selection is None:
                self.selection = {'version': 1, 'backend': 'files', 'profile_id': self.profile_id,
                                  'profile': self._context()}
                save_json(self.root / 'keys.mode.json', self.selection)
            return self.nostr
        committed_migration = (self.old_keys is not None and self.old_keys['backend'] == 'native'
                               and self.pending is not None and self.pending['migration']
                               and self.pending['state'] in ('COMMITTED', 'CLEANED')
                               and self.old_keys['nostr'] == self.pending['nostr']
                               and self.old_keys['master'] == self.pending['master'])
        require(not self.migration or migration_secret is not None or committed_migration,
                'explicit_key_migration_required')
        binding = self.old_keys['nostr'] if self.old_keys and self.old_keys['backend'] == 'native' else (
            self.pending['nostr'] if self.pending else None)
        fresh = binding is None
        proposed = None
        if fresh:
            require(pristine and existing_account is None, 'nonpristine_key_state')
            proposed = secrets.token_bytes(32)
            account = self._helper().derive_nostr(proposed)
            binding = self._candidate('nostr', account)
            self._new_pending(binding)
            self.created_now.add('nostr')
        self._binding(binding, 'nostr')
        require(existing_account is None or existing_account == binding['account'], 'profile_identity_mismatch')
        require(not self.pending or self.pending['migration'] or pristine or self.old_keys is not None, 'nonpristine_key_state')
        self._reserve(binding)
        supplied = _secret(migration_secret) if migration_secret is not None else proposed
        if supplied is not None:
            require(self._helper().derive_nostr(supplied) == binding['account'], 'profile_identity_mismatch')
        self.nostr = self._load_or_create(binding, 'nostr', supplied)
        return self.nostr

    def _load_or_create(self, binding, kind, supplied):
        try:
            raw = self._helper().load(binding['reference'])
        except SmokeError as error:
            if str(error) != 'missing' or supplied is None:
                raise
            require(self.migration or kind in self.created_now, 'pending_key_missing')
            self._helper().create(binding['reference'], self.name if kind == 'nostr' else 'marmish SQLCipher ' + self.profile_id[:16], supplied)
            raw = self._helper().load(binding['reference'])
        require(supplied is None or raw == supplied, 'native_key_mismatch')
        if kind == 'nostr':
            require(self._helper().derive_nostr(raw) == binding['account'], 'profile_identity_mismatch')
            result = NostrBinding(**binding, secret=raw)
        else:
            result = MasterBinding(**binding, secret=raw)
        if self.pending and kind not in self.pending['verified']:
            self.pending['verified'].append(kind)
            expected = {'nostr', 'master'} if self.pending['master'] else {'nostr'}
            if set(self.pending['verified']) == expected:
                self.pending['state'] = 'NATIVE_CREATED'
            self._journal()
        return result

    def prepare_master(self, engine_root, *, pristine=False, migration_secret=None):
        require(self.nostr is not None, 'nostr_key_not_prepared')
        engine_root = private_directory(engine_root)
        require(engine_root == self.root / 'engine', 'engine_root_mismatch')
        if self.backend == 'files':
            self.master = MasterBinding('files', self.profile_id, None, None)
            return self.master
        binding = (self.old_keys['master'] if self.old_keys and self.old_keys['backend'] == 'native'
                   and self.old_keys['master'] is not None else (self.pending['master'] if self.pending else None))
        supplied = _secret(migration_secret) if migration_secret is not None else None
        if binding is None:
            require(pristine and not self.migration, 'nonpristine_key_state')
            binding = self._candidate('master')
            supplied = secrets.token_bytes(32)
            self.created_now.add('master')
            if self.pending is None:
                self._new_pending(self.nostr.public())
                self.pending['verified'] = ['nostr']
            self.pending['master'] = binding
            self.pending['state'] = 'PENDING'
            self._journal()
        self._binding(binding, 'master')
        self.master = self._load_or_create(binding, 'master', supplied)
        return self.master

    def begin_migration(self, account):
        committed_recovery = (self.old_keys is not None and self.pending is not None
                              and self.pending['migration']
                              and self.old_keys['nostr'] == self.pending['nostr']
                              and self.old_keys['master'] == self.pending['master'])
        require(self.old_keys is None or self.old_keys['backend'] == 'files' or committed_recovery, 'already_native')
        require(self.storage != 'files', 'key_storage_conflict')
        self.migration = True
        self.backend = 'native'
        self.migration_account = public_key(account)

    def migration_targets(self, engine_root=None):
        require(self.migration, 'migration_not_started')
        if engine_root is not None:
            require(private_directory(engine_root) == self.root / 'engine', 'engine_root_mismatch')
        if self.pending is None:
            master = self._candidate('master') if engine_root is not None else None
            if engine_root is not None:
                require(private_directory(engine_root) == self.root / 'engine', 'engine_root_mismatch')
            self._new_pending(self._candidate('nostr', self.migration_account), migration=True, master=master)
        require(self.pending['migration'] and self.pending['nostr']['account'] == self.migration_account,
                'migration_identity_mismatch')
        require((self.pending['master'] is not None) == (engine_root is not None), 'migration_engine_mismatch')
        self._reserve(self.pending['nostr'])
        return {'nostr': dict(self.pending['nostr']), 'master': self.pending['master']}

    def load_migration_master(self):
        """Load only a committed exact native master for quiescence recovery."""
        require(self.pending is not None and self.pending['migration']
                and self.pending['state'] in ('COMMITTED', 'CLEANED')
                and self.old_keys is not None and self.old_keys['backend'] == 'native'
                and self.old_keys['nostr'] == self.pending['nostr']
                and self.old_keys['master'] == self.pending['master']
                and self.pending['master'] is not None, 'migration_not_committed')
        self._binding(self.pending['master'], 'master')
        return self._helper().load(self.pending['master']['reference'])

    def commit(self, value):
        require(self.nostr is not None and isinstance(value, dict), 'key_binding_not_prepared')
        require(value.get('account') == self.nostr.account or self.backend == 'files', 'profile_identity_mismatch')
        keys = {'version': 1, 'backend': self.backend, 'profile_id': self.profile_id,
                'nostr': self.nostr.public(), 'master': self.master.public() if self.master else (
                    self.old_keys['master'] if self.old_keys and self.old_keys['backend'] == 'native' else None)}
        if self.old_keys and self.old_keys['backend'] == 'native':
            require(self.old_keys['nostr'] == keys['nostr']
                    and (self.old_keys['master'] is None or self.old_keys['master'] == keys['master']), 'immutable_key_binding')
        if self.pending:
            require(self.pending['state'] in ('NATIVE_CREATED', 'COMMITTED', 'CLEANED'), 'native_keys_unverified')
        value = {**value, 'keys': keys}
        save_json(self.root / 'profile.json', value)
        self.value, self.old_keys = value, keys
        if self.pending:
            self.pending['state'] = 'COMMITTED'
            self._journal()
            if not self.migration:
                self.mark_cleaned()
        if self.catalog_lock is not None:
            os.close(self.catalog_lock)
            self.catalog_lock = None
        return value

    def mark_cleaned(self):
        require(self.pending is not None and self.pending['state'] in ('COMMITTED', 'CLEANED'), 'migration_not_committed')
        self.pending['state'] = 'CLEANED'
        self._journal()
        if self.backend == 'native' and self.selection is not None:
            require(self.migration, 'mixed_key_storage')
            (self.root / 'keys.mode.json').unlink()
            self.selection = None
        (self.root / 'keys.pending.json').unlink()
        fd = os.open(self.root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
        self.pending = None


def encode_nsec(secret):
    """Encode exactly32 private bytes as NIP-19 nsec, in memory only."""
    raw = _secret(secret)
    alphabet = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l'
    values, accumulator, bits = [], 0, 0
    for byte in raw:
        accumulator = accumulator << 8 | byte
        bits += 8
        while bits >= 5:
            bits -= 5
            values.append(accumulator >> bits & 31)
    if bits:
        values.append(accumulator << (5 - bits) & 31)
    hrp = 'nsec'
    expanded = [ord(c) >> 5 for c in hrp] + [0] + [ord(c) & 31 for c in hrp]
    check = 1
    for value in expanded + values + [0] * 6:
        high = check >> 25
        check = ((check & 0x1ffffff) << 5) ^ value
        for bit, generator in enumerate((0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3)):
            if high >> bit & 1:
                check ^= generator
    check ^= 1
    checksum = [(check >> 5 * (5 - i)) & 31 for i in range(6)]
    return hrp + '1' + ''.join(alphabet[x] for x in values + checksum)
