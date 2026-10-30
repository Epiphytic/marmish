/* Native proof fixture; never shipped or used by production. */
#include <sys/types.h>
#include <sys/wait.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include "marmish-lifetime-group.h"
#include "marmish-lifetime-cleanup.h"

static pid_t spawn(pid_t group, int *grant) {
    int ready[2], gate[2];
    if (pipe(ready)) return -1;
    if (pipe(gate)) { close(ready[0]); close(ready[1]); return -1; }
    pid_t child = fork();
    if (!child) {
        close(ready[0]); close(gate[1]);
        if (setpgid(0, group)) _exit(65);
        if (write(ready[1], "R", 1) != 1) _exit(66);
        close(ready[1]);
        char value;
        if (read(gate[0], &value, 1) != 1 || value != 'G') _exit(67);
        close(gate[0]);
        _exit(0);
    }
    close(ready[1]); close(gate[0]);
    char value;
    int published = child > 0 && read(ready[0], &value, 1) == 1 && value == 'R';
    close(ready[0]);
    if (!published) {
        close(gate[1]);
        if (child > 0) { kill(child, SIGKILL); while (waitpid(child, NULL, 0) < 0 && errno == EINTR) {} }
        return -1;
    }
    *grant = gate[1];
    return child;
}

static int leave_zombie(pid_t child, int grant) {
    if (write(grant, "G", 1) != 1) return 0;
    siginfo_t info;
    /* Blocking fixture synchronization; the Python caller has a timeout. */
    return waitid(P_PID, (id_t)child, &info, WEXITED | WNOWAIT) == 0 &&
        info.si_pid == child && exited(child) == 1;
}

int main(int argc, char **argv) {
    if (argc != 2 || signal(SIGPIPE, SIG_IGN) == SIG_ERR) return 64;
    int mode = atoi(argv[1]), error = 0;
    if (mode == 4) return lifetime_zombie_group(getpid(), &error) ? 65 : 0;
    if (mode < 0 || mode > 3) return 64;
    pid_t leader = -1, peer = -1;
    int leader_grant = -1, peer_grant = -1, result = 66;
    leader = spawn(0, &leader_grant);
    if (leader < 0) goto cleanup;
    if (mode == 2 || mode == 3) {
        peer = spawn(leader, &peer_grant);
        if (peer < 0) goto cleanup;
        if (mode == 3 && !leave_zombie(peer, peer_grant)) goto cleanup;
    }
    if (mode != 1 && !leave_zombie(leader, leader_grant)) goto cleanup;
    int proved = lifetime_zombie_group(leader, &error);
    if (proved != (mode == 0 || mode == 3)) { result = 68; goto cleanup; }
    result = 0;
cleanup:
    /* Retain the leader until all owned group/peer signals have been attempted. */
    if (peer > 0 && kill(peer, SIGKILL) < 0 && errno != ESRCH) result = 69;
    if (leader > 0 && finish(leader) != 0) result = 70;
    if (peer > 0) {
        while (waitpid(peer, NULL, 0) < 0) {
            if (errno != EINTR) { result = 71; break; }
        }
    }
    if (leader_grant >= 0) close(leader_grant);
    if (peer_grant >= 0) close(peer_grant);
    return result;
}
