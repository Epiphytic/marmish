"""Small fail-closed native identity and suspend-inclusive timing boundary."""
import ctypes
import functools
import os
from pathlib import Path
import re
import socket
import struct
import sys
import time

try:
    from .mdk_smoke import SmokeError, require
except ImportError:
    from mdk_smoke import SmokeError, require

_UUID = re.compile(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}")


def _uuid(value):
    require(isinstance(value, str) and _UUID.fullmatch(value), 'process_identity_failed')
    return value.lower()


class _BsdInfo(ctypes.Structure):
    _fields_ = ([(name, ctypes.c_uint32) for name in (
        'flags', 'status', 'xstatus', 'pid', 'ppid', 'uid', 'gid', 'ruid', 'rgid',
        'svuid', 'svgid', 'reserved')] +
        [('comm', ctypes.c_char * 16), ('name', ctypes.c_char * 32)] +
        [(name, ctypes.c_uint32) for name in ('nfiles', 'pgid', 'pjobc', 'tdev', 'tpgid')] +
        [('nice', ctypes.c_int32), ('seconds', ctypes.c_uint64), ('micros', ctypes.c_uint64)])


class _Timebase(ctypes.Structure):
    _fields_ = [('numer', ctypes.c_uint32), ('denom', ctypes.c_uint32)]


@functools.lru_cache(maxsize=1)
def _darwin():
    require(sys.platform == 'darwin', 'unsupported_platform')
    require(ctypes.sizeof(_BsdInfo) == 136 and _BsdInfo.seconds.offset == 120,
            'native_layout_mismatch')
    library = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
    library.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                                     ctypes.c_void_p, ctypes.c_int]
    library.proc_pidinfo.restype = ctypes.c_int
    library.sysctlbyname.argtypes = [ctypes.c_char_p, ctypes.c_void_p,
                                    ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p,
                                    ctypes.c_size_t]
    library.sysctlbyname.restype = ctypes.c_int
    library.mach_continuous_time.argtypes = []
    library.mach_continuous_time.restype = ctypes.c_uint64
    library.mach_timebase_info.argtypes = [ctypes.POINTER(_Timebase)]
    library.mach_timebase_info.restype = ctypes.c_int
    library.getpeereid.argtypes = [ctypes.c_int, ctypes.POINTER(ctypes.c_uint32),
                                  ctypes.POINTER(ctypes.c_uint32)]
    library.getpeereid.restype = ctypes.c_int
    base = _Timebase()
    require(library.mach_timebase_info(ctypes.byref(base)) == 0 and
            base.numer > 0 and base.denom > 0, 'native_clock_failed')
    return library, base


def continuous_time():
    """Seconds on a boot clock including suspend; never fall back silently."""
    if sys.platform == 'darwin':
        library, base = _darwin()
        return (library.mach_continuous_time() * base.numer // base.denom) / 1_000_000_000
    require(sys.platform.startswith('linux') and hasattr(time, 'CLOCK_BOOTTIME'),
            'unsupported_platform')
    return time.clock_gettime(time.CLOCK_BOOTTIME)


def _darwin_inspect(pid):
    library, _base = _darwin()
    data = _BsdInfo()
    ctypes.set_errno(0)
    count = library.proc_pidinfo(pid, 3, 0, ctypes.byref(data), ctypes.sizeof(data))
    error = ctypes.get_errno()
    if count == 0 and error == 3:  # ESRCH only proves absence.
        return None
    require(count == ctypes.sizeof(data) and data.pid == pid and
            data.seconds > 0 and data.micros < 1_000_000, 'process_identity_failed')
    start = data.seconds * 1_000_000 + data.micros
    require(start <= (1 << 64) - 1, 'process_identity_failed')
    value = ctypes.create_string_buffer(64)
    length = ctypes.c_size_t(len(value))
    require(library.sysctlbyname(b'kern.bootsessionuuid', value, ctypes.byref(length),
                               None, 0) == 0 and length.value in (36, 37),
            'process_identity_failed')
    raw = value.raw[:length.value]
    if len(raw) == 37:
        require(raw[-1:] == b'\0', 'process_identity_failed')
        raw = raw[:-1]
    return {'pid': pid, 'boot_id': _uuid(raw.decode('ascii')), 'start': start,
            'uid': data.uid, 'ruid': data.ruid}


def inspect_process(pid):
    """Darwin: None is proved absent; dict is inspected; errors are uncertain."""
    require(type(pid) is int and 0 < pid < (1 << 31), 'process_identity_failed')
    require(sys.platform == 'darwin', 'unsupported_platform')
    return _darwin_inspect(pid)


def _darwin_identity(pid):
    value = _darwin_inspect(pid)
    require(value is not None and value['uid'] == os.geteuid() and
            value['ruid'] == os.geteuid(), 'process_identity_failed')
    return {key: value[key] for key in ('pid', 'boot_id', 'start')}


def _linux_identity(pid):
    root = Path('/proc') / str(pid)
    info = root.stat()
    require(info.st_uid == os.geteuid(), 'process_identity_failed')
    # comm may contain whitespace and ')' characters; the last ')' delimits it.
    stat_line = (root / 'stat').read_text()
    fields = stat_line[stat_line.rfind(')') + 2:].split()
    require(int(stat_line.split(' ', 1)[0]) == pid and len(fields) > 19,
            'process_identity_failed')
    ticks = int(fields[19])
    require(ticks > 0, 'process_identity_failed')
    return {'pid': pid, 'boot_id': _uuid(Path('/proc/sys/kernel/random/boot_id').read_text().strip()),
            'start': ticks * 1_000_000 // os.sysconf('SC_CLK_TCK')}


def process_identity(pid):
    """Local UID + boot UUID + native process creation time, exact dict equality."""
    require(type(pid) is int and 0 < pid < (1 << 31), 'process_identity_failed')
    try:
        if sys.platform == 'darwin':
            return _darwin_identity(pid)
        require(sys.platform.startswith('linux'), 'unsupported_platform')
        return _linux_identity(pid)
    except (OSError, ValueError, UnicodeError, IndexError):
        raise SmokeError('process_identity_failed') from None


def verify_peer(connection, expected_uid, expected_identity=None):
    """Authenticate before writing any RPC bytes; Darwin also pins daemon PID."""
    if sys.platform == 'darwin':
        require(isinstance(expected_identity, dict), 'missing_peer_identity')
        library, _base = _darwin()
        uid, gid = ctypes.c_uint32(), ctypes.c_uint32()
        require(library.getpeereid(connection.fileno(), ctypes.byref(uid),
                                   ctypes.byref(gid)) == 0, 'peer_credentials_failed')
        require(uid.value == expected_uid, 'wrong_peer_uid')
        raw = connection.getsockopt(0, 2, 4)  # SOL_LOCAL, LOCAL_PEERPID
        require(len(raw) == 4, 'peer_credentials_failed')
        pid = struct.unpack('=i', raw)[0]
        require(process_identity(pid) == expected_identity, 'wrong_peer_identity')
    else:
        require(sys.platform.startswith('linux'), 'unsupported_platform')
        raw = connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
        require(len(raw) == 12, 'peer_credentials_failed')
        pid, uid, _gid = struct.unpack('=3i', raw)
        require(uid == expected_uid, 'wrong_peer_uid')
        if expected_identity is not None:
            require(process_identity(pid) == expected_identity, 'wrong_peer_identity')


def execution_marker():
    """Runner ownership tag only; never forwarded into command environments."""
    value = os.environ.get('EPIPHYTIC_EXECUTION_ID')
    if value is None:
        return {}
    require(re.fullmatch(r'[A-Za-z0-9_.:-]{1,160}', value) is not None,
            'invalid_execution_marker')
    return {'EPIPHYTIC_EXECUTION_ID': value}


def canonical_state_path(path):
    """Accept Darwin's two OS-owned aliases without hiding private symlinks."""
    path = Path(path).absolute()
    require('..' not in path.parts, 'unsafe_state_path')
    if sys.platform == 'darwin' and len(path.parts) > 1 and path.parts[1] in ('tmp', 'var'):
        alias = Path('/') / path.parts[1]
        try:
            info = alias.lstat()
        except OSError:
            raise SmokeError('unsafe_state_path') from None
        require(info.st_uid == 0 and alias.is_symlink() and
                os.readlink(alias) == 'private/' + path.parts[1], 'unsafe_state_path')
        path = Path('/private') / Path(*path.parts[1:])
    return path
