"""Portable tests of the production cleanup implementation, with fake syscalls."""
import errno
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import test_marmish_transport_macos as fixture


class LifetimeDiagnosticsTests(unittest.TestCase):
    def record(self, stage='waitid', error=errno.ECHILD):
        return dict(outcome='failed', category='native_lifetime_failed', stage=stage, errno=error)

    def test_closed_parser_order_bounds_and_redaction(self):
        valid = [self.record(), self.record(), self.record('kill_group'),
                 self.record('kill_leader'), self.record('waitpid')]
        raw = b''.join(json.dumps(r).encode() + b'\n' for r in valid)
        self.assertEqual(fixture.parse_lifetime_diagnostics(raw), valid)
        self.assertEqual(fixture.parse_lifetime_diagnostics(b''), [])
        self.assertEqual(fixture.parse_lifetime_diagnostics(json.dumps(self.record('waitpid', None)).encode()+b'\n'),
                         [self.record('waitpid', None)])
        invalid = [b'private-secret-path\n', b'\xff\n', b'{}\n', raw * 2, raw[:-1], b'x'*4097,
                   json.dumps(valid[-1]).encode()+b'\n'+json.dumps(valid[0]).encode()+b'\n',
                   b'\n', b'{"errno":1,"errno":2}\n', raw.replace(b'\n', b'\r\n')]
        for field, value in [('errno', True), ('errno', 0), ('errno', -1), ('errno', 4096),
                             ('stage', 'private-secret-path'), ('category', 'private-secret-path'),
                             ('extra', 'private-secret-path')]:
            record = self.record(); record[field] = value
            invalid.append(json.dumps(record).encode()+b'\n')
        for raw in invalid:
            with self.subTest(raw_length=len(raw)):
                with self.assertRaisesRegex(AssertionError, '^native_lifetime_diagnostic_invalid$'):
                    fixture.parse_lifetime_diagnostics(raw)

    def test_pipe_eof_size_and_deadline(self):
        reader, writer = os.pipe()
        with os.fdopen(reader, 'rb', buffering=0) as stream:
            try:
                with self.assertRaisesRegex(AssertionError, '^native_lifetime_diagnostic_invalid$'):
                    fixture.read_lifetime_diagnostics(stream, timeout=.01)
                os.write(writer, json.dumps(self.record()).encode()+b'\n')
            finally:
                os.close(writer)
            self.assertEqual(fixture.read_lifetime_diagnostics(stream), [self.record()])
        reader, writer = os.pipe()
        with os.fdopen(reader, 'rb', buffering=0) as stream:
            try:
                os.write(writer, b'x'*4097)
                with self.assertRaisesRegex(AssertionError, '^native_lifetime_diagnostic_invalid$'):
                    fixture.read_lifetime_diagnostics(stream)
            finally:
                os.close(writer)


class LifetimeCleanupTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        compiler = shutil.which('cc') or shutil.which('clang')
        if not compiler:
            raise unittest.SkipTest('portable native cleanup harness requires C compiler')
        cls.directory = tempfile.TemporaryDirectory(prefix='marmish-cleanup-harness-')
        cls.addClassCleanup(cls.directory.cleanup)
        root = Path(cls.directory.name)
        source = root / 'harness.c'
        source.write_text(r'''
#include <sys/types.h>
#include <sys/wait.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
static int mode, kills, waits, writes;
static int fake_kill(pid_t pid, int sig) {
    ++kills;
    if (sig != SIGKILL || (kills == 1 && pid != -123) || (kills == 2 && pid != 123)) abort();
    if ((mode == 1 && kills == 1) || (mode == 2 && kills == 2)) { errno = ESRCH; return -1; }
    if (mode == 3 || mode == 6) { errno = kills == 1 ? EPERM : EACCES; return -1; }
    if (mode == 10 && kills == 1) { errno = EPERM; return -1; }
    errno = EDOM; return 0;
}
static pid_t fake_waitpid(pid_t pid, int *status, int flags) {
    ++waits;
    if (pid != 123 || flags) abort();
    if (mode == 3 || mode == 6) { errno = ECHILD; return -1; }
    if (mode == 4 && waits == 1) { errno = EINTR; return -1; }
    *status = mode == 4 ? 7 << 8 : (mode == 9 ? SIGTERM : 0);
    errno = ERANGE; return pid;
}
static int fake_waitid(idtype_t type, id_t pid, siginfo_t *info, int flags) {
    if (type != P_PID || pid != 123 || flags != (WEXITED|WNOHANG|WNOWAIT)) abort();
    if (mode == 5) { errno = EINTR; return -1; }
    if (mode == 6 || mode == 7 || mode == 8) {
        errno = mode == 6 ? EINVAL : (mode == 7 ? 0 : 4096); return -1;
    }
    info->si_pid = 123; errno = EIO; return 0;
}
static ssize_t fake_write(int fd, const void *data, size_t size) {
    if (kills != 2 || !waits || fd != STDERR_FILENO || ++writes != 1) abort();
    return write(fd, data, size);
}
#define kill fake_kill
#define waitpid fake_waitpid
#define waitid fake_waitid
#define write fake_write
#include "marmish-lifetime-cleanup.h"
int main(int argc, char **argv) {
    if (argc != 2) return 64;
    mode = atoi(argv[1]);
    int observation = exited(123);
    if (mode == 6) (void)exited(123);
    int result = finish(123);
    printf("%d %d %d %d %d\n", result, observation, kills, waits, writes);
    return 0;
}
''')
        cls.executable = root / 'harness'
        header = Path(fixture.native.__file__).parent / 'native'
        subprocess.run([compiler, '-Wall', '-Wextra', '-Werror', '-I', str(header), str(source),
                        '-o', str(cls.executable)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    def test_production_cleanup_error_and_retry_contract(self):
        for mode in range(11):
            with self.subTest(mode=mode):
                result = subprocess.run([str(self.executable), str(mode)], check=True,
                                        capture_output=True, timeout=5)
                status, observation, kills, waits, writes = map(int, result.stdout.split())
                self.assertEqual(status, 70 if mode in (3, 6, 10) else (7 if mode == 4 else 0))
                self.assertEqual(observation, 0 if mode == 5 else (-1 if mode in (6, 7, 8) else 1))
                self.assertEqual((kills, waits), (2, 2 if mode == 4 else 1))
                records = fixture.parse_lifetime_diagnostics(result.stderr)
                expected = []
                if mode == 6:
                    expected += [('waitid', errno.EINVAL)] * 2
                if mode in (7, 8):
                    expected.append(('waitid', None))
                if mode in (3, 6):
                    expected += [('kill_group', errno.EPERM), ('kill_leader', errno.EACCES), ('waitpid', errno.ECHILD)]
                if mode == 10:
                    expected.append(('kill_group', errno.EPERM))
                self.assertEqual([(r['stage'], r['errno']) for r in records], expected)
                self.assertEqual(writes, int(bool(expected)))
