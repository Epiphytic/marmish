/* Owned daemon lifetime, independent of Python finally blocks. */
#include <sys/event.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <mach/mach_time.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>

static double elapsed(void) {
    mach_timebase_info_data_t scale;
    if (mach_timebase_info(&scale) != KERN_SUCCESS || !scale.denom) _exit(71);
    return (double)mach_continuous_time() * (double)scale.numer / (double)scale.denom / 1000000000.0;
}

#include "marmish-lifetime-cleanup.h"

int main(int argc, char **argv) {
    if (argc < 4) return 64;
    if (signal(SIGPIPE, SIG_IGN) == SIG_ERR) return 66;
    char *end = NULL;
    long fd = strtol(argv[1], &end, 10);
    if (!end || *end || fd < 3 || fd > 65535) return 64;
    long parent = strtol(argv[2], &end, 10);
    if (!end || *end || parent <= 1 || getppid() != parent) return 65;
    int queue = kqueue();
    if (queue < 0) return 66;
    struct kevent event;
    EV_SET(&event, (uintptr_t)parent, EVFILT_PROC, EV_ADD | EV_ENABLE | EV_ONESHOT, NOTE_EXIT, 0, NULL);
    if (kevent(queue, &event, 1, NULL, 0, NULL) < 0) return 66;
    int gate[2];
    if (pipe(gate)) return 66;
    pid_t child = fork();
    if (child < 0) return 67;
    if (!child) {
        close((int)fd); close(queue); close(gate[1]);
        if (signal(SIGPIPE, SIG_DFL) == SIG_ERR) _exit(68);
        if (setsid() < 0) _exit(68);
        char grant;
        if (read(gate[0], &grant, 1) != 1 || grant != 'G') _exit(68);
        close(gate[0]);
        int quiet = open("/dev/null", O_RDWR);
        if (quiet < 0 || dup2(quiet, STDOUT_FILENO) < 0 || dup2(quiet, STDERR_FILENO) < 0) _exit(68);
        if (quiet > 2) close(quiet);
        execv(argv[3], &argv[3]); _exit(69);
    }
    close(gate[0]);
    /* Only the sandbox child retains the bootstrap reader. */
    int null_input = open("/dev/null", O_RDONLY);
    if (null_input < 0 || dup2(null_input, STDIN_FILENO) < 0) {
        kill(child, SIGTERM); close(gate[1]); return finish(child);
    }
    struct stat actual_input, expected_input;
    if (fstat(STDIN_FILENO, &actual_input) || fstat(null_input, &expected_input) ||
            !S_ISCHR(actual_input.st_mode) || actual_input.st_rdev != expected_input.st_rdev) {
        kill(child, SIGTERM); close(gate[1]); return finish(child);
    }
    if (null_input > 2) close(null_input);
    printf("%ld\n", (long)child); fflush(stdout);
    /* The child cannot execute until Python has durably recorded its identity.
       A killed shim closes gate[1], making the blocked child exit on EOF. */
    char grant;
    if (read((int)fd, &grant, 1) == 1 && grant == 'G') {
        if (write(gate[1], &grant, 1) != 1) kill(child, SIGTERM);
    } else kill(child, SIGTERM);
    close(gate[1]);
    for (;;) {
        int result = exited(child);
        if (result == 1) return finish(child);
        if (result < 0) break;
        struct pollfd channels[2] = {{(int)fd, POLLIN | POLLHUP, 0}, {queue, POLLIN, 0}};
        if (poll(channels, 2, 100) < 0 && errno != EINTR) break;
        if (channels[0].revents || channels[1].revents || getppid() != parent) break;
    }
    kill(-child, SIGTERM);
    kill(child, SIGTERM);
    double deadline = elapsed() + 2.0;
    while (elapsed() < deadline) {
        int result = exited(child);
        if (result != 0) break;
        usleep(20000);
    }
    /* Even an exited leader may have live group members. Reap only afterward. */
    return finish(child);
}
