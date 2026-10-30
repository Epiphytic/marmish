/* Darwin-only bounded proof; no signal, reap or diagnostic side effects. */
#ifndef MARMISH_LIFETIME_GROUP_H
#define MARMISH_LIFETIME_GROUP_H
#include <sys/types.h>
#include <sys/wait.h>
#include <sys/proc.h>
#include <libproc.h>
#include <errno.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>
#ifdef __APPLE__
_Static_assert(sizeof(struct proc_bsdinfo) == 136, "unsupported BSD info ABI");
#endif
#define LIFETIME_GROUP_LIMIT 256
struct lifetime_identity {
    uint64_t seconds, micros;
    uint32_t pid, ppid, pgid, uid, ruid;
};

static int lifetime_same_identity(const struct lifetime_identity *a, const struct lifetime_identity *b) {
    return a->seconds == b->seconds && a->micros == b->micros && a->pid == b->pid &&
        a->ppid == b->ppid && a->pgid == b->pgid && a->uid == b->uid && a->ruid == b->ruid;
}

static int lifetime_held_zombie(pid_t leader, int *error) {
    siginfo_t info;
    memset(&info, 0, sizeof(info));
    errno = 0;
    int result = waitid(P_PID, (id_t)leader, &info, WEXITED | WNOHANG | WNOWAIT);
    int saved = errno;
    if (result < 0) { *error = saved; return 0; }
    return result == 0 && info.si_pid == leader &&
        (info.si_code == CLD_EXITED || info.si_code == CLD_KILLED || info.si_code == CLD_DUMPED);
}

static int lifetime_zombie_info(pid_t pid, pid_t group, uid_t owner,
                                struct lifetime_identity *identity, int *error) {
    struct proc_bsdinfo info;
    memset(&info, 0, sizeof(info));
    errno = 0;
    int count = proc_pidinfo(pid, PROC_PIDTBSDINFO, 1, &info, sizeof(info));
    int saved = errno;
    if (count != sizeof(info)) { if (count <= 0) *error = saved; return 0; }
    if (info.pbi_pid != (uint32_t)pid || info.pbi_pgid != (uint32_t)group ||
        info.pbi_uid != owner || info.pbi_ruid != owner || info.pbi_status != SZOMB ||
        !info.pbi_start_tvsec || info.pbi_start_tvusec >= 1000000) return 0;
    identity->seconds = info.pbi_start_tvsec;
    identity->micros = info.pbi_start_tvusec;
    identity->pid = info.pbi_pid;
    identity->ppid = info.pbi_ppid;
    identity->pgid = info.pbi_pgid;
    identity->uid = info.pbi_uid;
    identity->ruid = info.pbi_ruid;
    return 1;
}

static int lifetime_group_members(pid_t leader, pid_t *pids, int *error) {
    const int capacity = (LIFETIME_GROUP_LIMIT + 1) * sizeof(*pids);
    errno = 0;
    int count = proc_listpids(PROC_PGRP_ONLY, (uint32_t)leader, pids, capacity);
    int saved = errno;
    if (count <= 0) { *error = saved; return 0; }
    if (count >= capacity || count % sizeof(*pids)) return 0;
    int length = count / sizeof(*pids);
    int found = 0;
    for (int i = 0; i < length; ++i) {
        if (pids[i] <= 0) return 0;
        if (pids[i] == leader) ++found;
        pid_t value = pids[i];
        int j = i;
        while (j > 0 && pids[j - 1] > value) { pids[j] = pids[j - 1]; --j; }
        pids[j] = value;
    }
    if (found != 1) return 0;
    for (int i = 1; i < length; ++i) if (pids[i - 1] == pids[i]) return 0;
    return length;
}

static int lifetime_zombie_group(pid_t leader, int *error) {
    *error = 0;
    uid_t owner = getuid();
    if (leader <= 1 || !owner || owner != geteuid() || !lifetime_held_zombie(leader, error)) return 0;
    struct lifetime_identity original, current, members[LIFETIME_GROUP_LIMIT];
    if (!lifetime_zombie_info(leader, leader, owner, &original, error) ||
        original.ppid != (uint32_t)getpid()) return 0;
    pid_t first[LIFETIME_GROUP_LIMIT + 1], second[LIFETIME_GROUP_LIMIT + 1];
    int count = lifetime_group_members(leader, first, error);
    if (!count) return 0;
    for (int i = 0; i < count; ++i) {
        if (!lifetime_zombie_info(first[i], leader, owner, &members[i], error)) return 0;
        if (first[i] == leader && !lifetime_same_identity(&original, &members[i])) return 0;
    }
    int again = lifetime_group_members(leader, second, error);
    if (again != count) return 0;
    for (int i = 0; i < count; ++i) {
        if (first[i] != second[i] || !lifetime_zombie_info(second[i], leader, owner, &current, error) ||
            !lifetime_same_identity(&members[i], &current)) return 0;
    }
    return lifetime_held_zombie(leader, error) &&
        lifetime_zombie_info(leader, leader, owner, &current, error) &&
        lifetime_same_identity(&original, &current);
}
#endif
