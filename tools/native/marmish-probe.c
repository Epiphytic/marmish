/* No payloads: exit zero means the tested capability was available. */
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/stat.h>
#include <sys/mount.h>
#include <errno.h>
#include <arpa/inet.h>
#include <fcntl.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* Closed storage result: no paths, names or native structures leave this mode. */
static int storage(const char *path) {
    int result = 0, error = 0;
    errno = 0;
    int fd = open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0) { error = errno; result = 3; }
    else {
        struct stat metadata;
        struct statfs filesystem;
        memset(&metadata, 0, sizeof(metadata));
        memset(&filesystem, 0, sizeof(filesystem));
        errno = 0;
        if (fstat(fd, &metadata) != 0) { error = errno; result = 4; }
        else if (!S_ISDIR(metadata.st_mode) || !geteuid() || metadata.st_uid != geteuid() ||
                 (metadata.st_mode & 07777) != 0700) result = 7;
        else {
            errno = 0;
            if (fstatfs(fd, &filesystem) != 0) { error = errno; result = 5; }
            else if (!memchr(filesystem.f_fstypename, 0, sizeof(filesystem.f_fstypename)) ||
                     strcmp(filesystem.f_fstypename, "apfs") || !(filesystem.f_flags & MNT_LOCAL)) result = 2;
        }
        errno = 0;
        int closed = close(fd);
        int close_error = errno;
        if (closed != 0 && !result) { error = close_error; result = 6; }
    }
    if (result >= 3 && result <= 6) printf("%d\n", error > 0 && error <= 4095 ? error : 0);
    return result;
}

int main(int argc, char **argv) {
    if (argc < 3) return 64;
    if (!strcmp(argv[1], "storage")) return argc == 3 ? storage(argv[2]) : 64;
    int fd = -1;
    if (!strcmp(argv[1], "read") || !strcmp(argv[1], "write")) {
        fd = open(argv[2], !strcmp(argv[1], "read") ? O_RDONLY : O_WRONLY);
    } else if (!strcmp(argv[1], "create")) {
        fd = open(argv[2], O_WRONLY | O_CREAT | O_EXCL, 0600);
    } else if (!strcmp(argv[1], "truncate")) {
        fd = open(argv[2], O_WRONLY | O_TRUNC);
    } else if (!strcmp(argv[1], "unlink")) {
        return unlink(argv[2]) == 0 ? 0 : 2;
    } else if (argc == 4 && !strcmp(argv[1], "rename")) {
        return rename(argv[2], argv[3]) == 0 ? 0 : 2;
    } else if (!strcmp(argv[1], "dns")) {
        struct addrinfo *result = NULL;
        int rc = getaddrinfo(argv[2], "443", NULL, &result);
        if (result) freeaddrinfo(result);
        return rc == 0 ? 0 : 2;
    } else if (argc == 4 && !strcmp(argv[1], "unix-staged")) {
        char directory[4096], final[4096];
        struct sockaddr_un address = {0}; address.sun_family = AF_UNIX;
        int n = snprintf(directory, sizeof(directory), "%s/.sock.%ld.%s", argv[2], (long)getpid(), argv[3]);
        if (n < 0 || (size_t)n >= sizeof(directory)) return 64;
        n = snprintf(address.sun_path, sizeof(address.sun_path), "%s/%s", directory, argv[3]);
        if (n < 0 || (size_t)n >= sizeof(address.sun_path)) return 64;
        n = snprintf(final, sizeof(final), "%s/%s", argv[2], argv[3]);
        if (n < 0 || (size_t)n >= sizeof(final)) return 64;
        if (mkdir(directory, 0700)) return 2;
        fd = socket(AF_UNIX, SOCK_STREAM, 0);
        int ok = fd >= 0 && !bind(fd, (struct sockaddr *)&address, sizeof(address)) &&
                 !listen(fd, 1) && !link(address.sun_path, final);
        unlink(final); unlink(address.sun_path); rmdir(directory);
        if (fd >= 0) close(fd);
        return ok ? 0 : 2;
    } else if (!strcmp(argv[1], "unix") || !strcmp(argv[1], "unix-bind")) {
        struct sockaddr_un address = {0}; address.sun_family = AF_UNIX;
        if (strlen(argv[2]) >= sizeof(address.sun_path)) return 64;
        strcpy(address.sun_path, argv[2]); fd = socket(AF_UNIX, SOCK_STREAM, 0);
        if (fd >= 0) {
            int rc = !strcmp(argv[1], "unix") ? connect(fd, (struct sockaddr *)&address, sizeof(address))
                : bind(fd, (struct sockaddr *)&address, sizeof(address));
            if (rc) { close(fd); fd = -1; }
            else if (!strcmp(argv[1], "unix-bind") && listen(fd, 1)) { close(fd); fd = -1; }
        }
    } else if (argc == 4 && (!strcmp(argv[1], "tcp") || !strcmp(argv[1], "udp"))) {
        int family = strchr(argv[2], ':') ? AF_INET6 : AF_INET;
        int type = !strcmp(argv[1], "udp") ? SOCK_DGRAM : SOCK_STREAM;
        fd = socket(family, type, 0);
        struct sockaddr_storage storage = {0}; socklen_t size;
        if (family == AF_INET) {
            struct sockaddr_in *a = (void *)&storage; a->sin_family = AF_INET;
            a->sin_port = htons((unsigned short)atoi(argv[3])); inet_pton(family, argv[2], &a->sin_addr); size = sizeof(*a);
        } else {
            struct sockaddr_in6 *a = (void *)&storage; a->sin6_family = AF_INET6;
            a->sin6_port = htons((unsigned short)atoi(argv[3])); inet_pton(family, argv[2], &a->sin6_addr); size = sizeof(*a);
        }
        if (fd >= 0 && connect(fd, (struct sockaddr *)&storage, size)) { close(fd); fd = -1; }
        if (fd >= 0 && type == SOCK_DGRAM && send(fd, "x", 1, 0) != 1) { close(fd); fd = -1; }
    } else return 64;
    if (fd < 0) return 2;
    close(fd); return 0;
}
