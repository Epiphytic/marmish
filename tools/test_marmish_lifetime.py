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
        valid = [self.record(), self.record(), self.record('group_inspection'), self.record('kill_group'),
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
static int mode, kills, waits, writes, proofs;
static int lifetime_zombie_group(pid_t pid, int *error) {
    if (pid != 123) abort();
    ++proofs; *error = 13; errno = EBUSY; return mode == 11 || mode == 12 || mode == 13;
}
static int fake_kill(pid_t pid, int sig) {
    ++kills;
    if (sig != SIGKILL || (kills == 1 && pid != -123) || (kills == 2 && pid != 123)) abort();
    if ((mode == 1 && kills == 1) || (mode == 2 && kills == 2)) { errno = ESRCH; return -1; }
    if (mode == 3 || mode == 6) { errno = kills == 1 ? EPERM : EACCES; return -1; }
    if ((mode == 10 || mode == 11 || mode == 12 || mode == 13) && kills == 1) { errno = EPERM; return -1; }
    errno = EDOM; return 0;
}
static pid_t fake_waitpid(pid_t pid, int *status, int flags) {
    ++waits;
    if (pid != 123 || flags) abort();
    if (mode == 3 || mode == 6 || mode == 12) { errno = ECHILD; return -1; }
    if (mode == 4 && waits == 1) { errno = EINTR; return -1; }
    *status = mode == 4 || mode == 13 ? 7 << 8 : (mode == 9 ? SIGTERM : 0);
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
    printf("%d %d %d %d %d %d\n", result, observation, kills, waits, writes, proofs);
    return 0;
}
''')
        cls.executable = root / 'harness'
        header = Path(fixture.native.__file__).parent / 'native'
        subprocess.run([compiler, '-Wall', '-Wextra', '-Werror', '-I', str(header), str(source),
                        '-o', str(cls.executable)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    def test_production_cleanup_error_and_retry_contract(self):
        for mode in range(14):
            with self.subTest(mode=mode):
                result = subprocess.run([str(self.executable), str(mode)], check=True,
                                        capture_output=True, timeout=5)
                status, observation, kills, waits, writes, proofs = map(int, result.stdout.split())
                self.assertEqual(status, 70 if mode in (3, 6, 10, 12) else (7 if mode in (4, 13) else 0))
                self.assertEqual(observation, 0 if mode == 5 else (-1 if mode in (6, 7, 8) else 1))
                self.assertEqual((kills, waits), (2, 2 if mode == 4 else 1))
                records = fixture.parse_lifetime_diagnostics(result.stderr)
                expected = []
                if mode == 6:
                    expected += [('waitid', errno.EINVAL)] * 2
                if mode in (7, 8):
                    expected.append(('waitid', None))
                if mode in (3, 6):
                    expected += [('group_inspection', 13), ('kill_group', errno.EPERM), ('kill_leader', errno.EACCES), ('waitpid', errno.ECHILD)]
                if mode == 10:
                    expected += [('group_inspection', 13), ('kill_group', errno.EPERM)]
                if mode == 12:
                    expected.append(('waitpid', errno.ECHILD))
                self.assertEqual(proofs, int(mode in (3, 6, 10, 11, 12, 13)))
                self.assertEqual([(r['stage'], r['errno']) for r in records], expected)
                self.assertEqual(writes, int(bool(expected)))


class ZombieGroupProofTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        compiler = shutil.which('cc') or shutil.which('clang')
        if not compiler:
            raise unittest.SkipTest('portable group contract harness requires C compiler')
        cls.directory = tempfile.TemporaryDirectory(prefix='marmish-group-harness-')
        cls.addClassCleanup(cls.directory.cleanup)
        root = Path(cls.directory.name)
        (root / 'sys').mkdir()
        (root / 'sys/proc.h').write_text('#define SZOMB 5\n')
        (root / 'libproc.h').write_text('''
#ifndef TEST_LIBPROC_H
#define TEST_LIBPROC_H
#include <stdint.h>
#include <sys/types.h>
#define PROC_PIDTBSDINFO 3
#define PROC_PGRP_ONLY 2
/* Test data shape only, not an assertion about the Darwin ABI. */
struct proc_bsdinfo {
 uint32_t pbi_pid, pbi_ppid, pbi_uid, pbi_ruid, pbi_status, pbi_pgid;
 uint64_t pbi_start_tvsec, pbi_start_tvusec;
};
#endif
''')
        source = root / 'group.c'
        source.write_text(r'''
#include <sys/types.h>
#include <sys/wait.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <libproc.h>
static int mode, infos, lists, checks, leaders, peers;
static uid_t fake_uid(void) { return mode == 28 ? 0 : 1001; }
static uid_t fake_euid(void) { return mode == 29 ? 1002 : fake_uid(); }
static pid_t fake_pid(void) { return 77; }
static int fake_waitid(idtype_t type, id_t pid, siginfo_t *info, int flags) {
 ++checks;
 if (type != P_PID || pid != 123 || flags != (WEXITED|WNOHANG|WNOWAIT)) abort();
 errno = EDOM;
 if (mode == 1 || mode == 49 || (mode == 41 && checks == 2)) { errno = mode == 1 ? EPERM : EINTR; return -1; }
 if (mode == 37) return 1;
 info->si_pid = mode == 2 || (mode == 27 && checks == 2) ? 0 : 123;
 info->si_code = mode == 3 ? CLD_STOPPED : (mode == 47 ? CLD_KILLED : (mode == 48 ? CLD_DUMPED : CLD_EXITED));
 return 0;
}
static int fake_info(int pid, int flavor, uint64_t arg, void *buffer, int size) {
 ++infos;
 if (flavor != PROC_PIDTBSDINFO || arg != 1 || size != sizeof(struct proc_bsdinfo)) abort();
 struct proc_bsdinfo *p = buffer;
 if (pid == 123) ++leaders; else if (pid >= 124 && pid <= 378) ++peers; else abort();
 errno = ERANGE;
 if ((mode == 4 && infos == 1) || (mode == 24 && pid == 124) || (mode == 46 && pid == 124 && peers == 2)) { errno = ESRCH; return 0; }
 if (mode == 5) return size - 1;
 p->pbi_pid = (uint32_t)pid; p->pbi_ppid = pid == 123 ? 77 : 88;
 p->pbi_uid = 1001; p->pbi_ruid = 1001; p->pbi_pgid = 123; p->pbi_status = 5;
 p->pbi_start_tvsec = (uint64_t)pid; p->pbi_start_tvusec = 12;
 if (mode == 6) p->pbi_pid++;
 if (mode == 7) p->pbi_pgid++;
 if (mode == 8) p->pbi_ppid++;
 if (mode == 9) p->pbi_uid++;
 if (mode == 10) p->pbi_ruid++;
 if (mode == 11) p->pbi_status = 2;
 if (mode == 12) p->pbi_start_tvsec = 0;
 if (mode == 13) p->pbi_start_tvusec = 1000000;
 if (mode == 25 && pid == 124 && peers == 2) p->pbi_start_tvsec++;
 if (mode == 26 && pid == 123 && leaders == 4) p->pbi_start_tvsec++;
 if (mode == 30 && pid == 124) p->pbi_pid++;
 if (mode == 31 && pid == 124) p->pbi_pgid++;
 if (mode == 32 && pid == 124) p->pbi_uid++;
 if (mode == 33 && pid == 124) p->pbi_ruid++;
 if (mode == 34 && pid == 124) p->pbi_status = 2;
 if (mode == 35 && pid == 124) p->pbi_start_tvsec = 0;
 if (mode == 36 && pid == 124) p->pbi_start_tvusec = 1000000;
 if (mode == 38 && pid == 124) p->pbi_ppid = 999;
 if (mode == 42 && pid == 123 && leaders == 4) p->pbi_ppid++;
 return size;
}
static int fake_list(uint32_t type, uint32_t group, void *buffer, int capacity) {
 ++lists;
 if (type != PROC_PGRP_ONLY || group != 123 || capacity != 257 * sizeof(pid_t)) abort();
 errno = EBUSY;
 if (mode == 14 || mode == 15 || (mode == 40 && lists == 2)) {
  errno = mode == 14 ? EACCES : (mode == 15 ? EIO : EAGAIN); return mode == 15 ? -1 : 0;
 }
 if (mode == 16 || mode == 45) return capacity;
 if (mode == 17) return 5;
 if (mode == 18) return capacity + sizeof(pid_t);
 pid_t *p = buffer;
 if (mode == 44) { for (int i = 0; i < 256; ++i) p[i] = 123 + i; return 256 * sizeof(pid_t); }
 p[0] = 124; p[1] = 123;
 if (mode == 19) p[0] = 0;
 if (mode == 43) { p[2] = 123; return 3 * sizeof(pid_t); }
 if (mode == 20) { p[2] = 124; return 3 * sizeof(pid_t); }
 if (mode == 21 || (mode == 23 && lists == 2)) return sizeof(pid_t);
 if (mode == 22 && lists == 2) p[0] = 125;
 if (mode == 39 && lists == 2) { p[0] = 123; p[1] = 124; }
 return 2 * sizeof(pid_t);
}
#define getuid fake_uid
#define geteuid fake_euid
#define getpid fake_pid
#define waitid fake_waitid
#define proc_pidinfo fake_info
#define proc_listpids fake_list
#include "marmish-lifetime-group.h"
int main(int argc, char **argv) {
 if (argc != 2) return 64;
 mode = atoi(argv[1]); int error = 12345;
 int result = lifetime_zombie_group(123, &error);
 printf("%d %d %d %d %d\n", result, error, infos, lists, checks);
 return 0;
}
''')
        cls.executable = root / 'group'
        header = Path(fixture.native.__file__).parent / 'native'
        subprocess.run([compiler, '-Wall', '-Wextra', '-Werror', '-I', str(root), '-I', str(header),
                        str(source), '-o', str(cls.executable)], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    def test_complete_bounded_group_and_native_identity_contract(self):
        native_errors = {1: errno.EPERM, 4: errno.ESRCH, 14: errno.EACCES, 15: errno.EIO,
                         24: errno.ESRCH, 40: errno.EAGAIN, 41: errno.EINTR,
                         46: errno.ESRCH, 49: errno.EINTR}
        for mode in range(50):
            with self.subTest(mode=mode):
                output = subprocess.check_output([str(self.executable), str(mode)], timeout=5)
                accepted, error, infos, lists, checks = map(int, output.split())
                self.assertEqual(accepted, int(mode in (0, 38, 39, 44, 47, 48)))
                self.assertEqual(error, native_errors.get(mode, 0))
                self.assertLessEqual(infos, 514)
                self.assertLessEqual(lists, 2)
                self.assertLessEqual(checks, 2)
                if accepted:
                    self.assertEqual((infos, lists, checks), (514 if mode == 44 else 6, 2, 2))
