/* Native lifetime fixture; never shipped with the daemon artifact. */
#include <sys/types.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void report(const char *path, long value) {
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0) _exit(64);
    char text[64];
    int length = snprintf(text, sizeof(text), "%ld\n", value);
    if (write(fd, text, (size_t)length) != length) _exit(65);
    close(fd);
}
int main(int argc, char **argv) {
    if (argc != 3) return 64;
    if (!strcmp(argv[1], "stdin")) {
        unsigned char input[73];
        size_t received = 0;
        ssize_t count;
        while (received < sizeof(input) && (count = read(0, input + received, sizeof(input) - received)) > 0)
            received += (size_t)count;
        if (received != 72 || memcmp(input, "MRMNST01", 8)) return 67;
        for (size_t i = 8; i < 72; ++i) if (input[i] != (unsigned char)(i - 8)) return 68;
        report(argv[2], (long)received);
        return 0;
    }
    if (!strcmp(argv[1], "signal")) {
        struct sigaction action;
        if (sigaction(SIGPIPE, NULL, &action)) return 65;
        report(argv[2], action.sa_handler == SIG_DFL);
        return 0;
    }
    if (!strcmp(argv[1], "wait")) signal(SIGTERM, SIG_IGN);
    pid_t child = fork();
    if (child < 0) return 66;
    if (!child) {
        signal(SIGTERM, SIG_IGN);
        for (;;) pause();
    }
    report(argv[2], child);
    if (!strcmp(argv[1], "fork")) return 0;
    for (;;) pause();
}
