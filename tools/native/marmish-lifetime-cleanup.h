/* Internal shared cleanup implementation; tests substitute only native calls. */
#ifndef MARMISH_LIFETIME_CLEANUP_H
#define MARMISH_LIFETIME_CLEANUP_H
#include <sys/types.h>
#include <sys/wait.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

enum lifetime_stage { LIFETIME_WAITID, LIFETIME_GROUP_INSPECTION, LIFETIME_KILL_GROUP, LIFETIME_KILL_LEADER, LIFETIME_WAITPID };
static struct { enum lifetime_stage stage; int error; } lifetime_errors[6];
static size_t lifetime_error_count;

static void lifetime_record(enum lifetime_stage stage, int error) {
    if (lifetime_error_count < sizeof(lifetime_errors) / sizeof(lifetime_errors[0])) {
        lifetime_errors[lifetime_error_count].stage = stage;
        lifetime_errors[lifetime_error_count++].error = error;
    }
}

/* Called only after every final signal and wait attempt, never during cleanup. */
static void lifetime_emit(void) {
    static const char *const stages[] = {"waitid", "group_inspection", "kill_group", "kill_leader", "waitpid"};
    char output[1024];
    size_t used = 0;
    for (size_t i = 0; i < lifetime_error_count; ++i) {
        char number[16];
        int error = lifetime_errors[i].error;
        if (error > 0 && error <= 4095) snprintf(number, sizeof(number), "%d", error);
        else snprintf(number, sizeof(number), "null");
        int length = snprintf(output + used, sizeof(output) - used,
            "{\"outcome\":\"failed\",\"category\":\"native_lifetime_failed\",\"stage\":\"%s\",\"errno\":%s}\n",
            stages[lifetime_errors[i].stage], number);
        if (length < 0 || (size_t)length >= sizeof(output) - used) return;
        used += (size_t)length;
    }
    if (used) (void)write(STDERR_FILENO, output, used);
}

/* Observe without reaping: the leader holds its pid/pgid until final signals. */
static int exited(pid_t child) {
    siginfo_t info;
    memset(&info, 0, sizeof(info));
    if (waitid(P_PID, (id_t)child, &info, WEXITED | WNOHANG | WNOWAIT) < 0) {
        int error = errno;
        if (error == EINTR) return 0;
        lifetime_record(LIFETIME_WAITID, error);
        return -1;
    }
    return info.si_pid == child;
}

static int finish(pid_t child) {
    /* ESRCH is fine; no group may remain. Never signal after reaping. */
    int failed = 0;
    if (kill(-child, SIGKILL) < 0) {
        int error = errno;
        if (error != ESRCH) {
            int inspection = 0;
            if (error != EPERM || !lifetime_zombie_group(child, &inspection)) {
                if (error == EPERM) lifetime_record(LIFETIME_GROUP_INSPECTION, inspection);
                lifetime_record(LIFETIME_KILL_GROUP, error);
                failed = 1;
            }
        }
    }
    if (kill(child, SIGKILL) < 0) {
        int error = errno;
        if (error != ESRCH) { lifetime_record(LIFETIME_KILL_LEADER, error); failed = 1; }
    }
    int status;
    while (waitpid(child, &status, 0) < 0) {
        int error = errno;
        if (error != EINTR) {
            lifetime_record(LIFETIME_WAITPID, error);
            lifetime_emit();
            return 70;
        }
    }
    lifetime_emit();
    return failed ? 70 : (WIFEXITED(status) ? WEXITSTATUS(status) : 0);
}
#endif
