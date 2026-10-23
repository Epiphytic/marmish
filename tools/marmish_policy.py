"""Trusted, bounded server command-policy configuration (never remote input)."""
import os
from pathlib import Path
import stat

from marmish_rpc import RpcError, public_key, strict_json
from mdk_smoke import SmokeError

FILE_CAP = 64 * 1024
KEY_CAP = 128
PATTERN_CAP = 256
PATTERN_BYTES = 1024


def _require(value):
    if not value:
        raise SmokeError('invalid_command_policy')


def _pattern(value):
    _require(isinstance(value, str) and value.startswith('/'))
    _require(len(value.encode('utf-8')) <= PATTERN_BYTES
             and not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value))
    index = 0
    while index < len(value):
        if value[index] == '\\':
            index += 1
            _require(index < len(value) and value[index] in ('*', '\\'))
        index += 1
    return value


def _restrictions(value, count):
    _require(isinstance(value, dict) and set(value) <= {'allow_exec', 'deny_exec'})
    result = {}
    for name, patterns in value.items():
        _require(isinstance(patterns, list))
        count[0] += len(patterns)
        _require(count[0] <= PATTERN_CAP)
        result[name] = [_pattern(pattern) for pattern in patterns]
    return result


def validate_policy(value, allowed):
    _require(isinstance(value, dict) and set(value) <= {'version', 'defaults', 'keys'})
    _require(type(value.get('version')) is int and value['version'] == 1)
    count = [0]
    defaults = _restrictions(value.get('defaults', {}), count)
    keys = value.get('keys', [])
    _require(isinstance(keys, list) and len(keys) <= KEY_CAP)
    result = []
    for entry in keys:
        _require(isinstance(entry, dict) and set(entry) == {'principal', 'restrictions'})
        principal = public_key(entry['principal'])
        _require(principal in allowed)
        result.append({'principal': principal,
                       'restrictions': _restrictions(entry['restrictions'], count)})
    return {'version': 1, 'defaults': defaults, 'keys': result}


def load_policy(path, allowed):
    if path is None:
        return None
    try:
        fd = os.open(Path(path), os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, 'rb') as source:
            meta = os.fstat(source.fileno())
            _require(stat.S_ISREG(meta.st_mode) and meta.st_uid == os.geteuid()
                     and meta.st_mode & 0o022 == 0)
            data = source.read(FILE_CAP + 1)
        _require(len(data) <= FILE_CAP)
        # Decode explicitly: json.loads(bytes) otherwise accepts a UTF BOM.
        text = data.decode('utf-8')
        _require(not text.startswith('\ufeff'))
        return validate_policy(strict_json(text), set(allowed))
    except (OSError, ValueError, TypeError, UnicodeError, RecursionError, RpcError, SmokeError):
        raise SmokeError('invalid_command_policy') from None
