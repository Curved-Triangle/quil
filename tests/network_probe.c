#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static void quil_probe(const char *name) {
    const char *path = getenv("QUIL_NETWORK_PROBE_PATH");
    if (path == NULL) {
        return;
    }
    int fd = open(path, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd >= 0) {
        (void)write(fd, name, strlen(name));
        (void)write(fd, "\n", 1);
        (void)close(fd);
    }
}

static int quil_connect(int socket, const struct sockaddr *address, socklen_t length) {
    (void)socket;
    (void)address;
    (void)length;
    quil_probe("connect");
    errno = EACCES;
    return -1;
}

static ssize_t quil_sendto(int socket, const void *buffer, size_t length, int flags,
                           const struct sockaddr *address, socklen_t address_length) {
    (void)socket;
    (void)buffer;
    (void)length;
    (void)flags;
    (void)address;
    (void)address_length;
    quil_probe("sendto");
    errno = EACCES;
    return -1;
}

static int quil_getaddrinfo(const char *host, const char *service,
                            const struct addrinfo *hints, struct addrinfo **result) {
    (void)host;
    (void)service;
    (void)hints;
    (void)result;
    quil_probe("getaddrinfo");
    return EAI_FAIL;
}

struct quil_interpose {
    const void *replacement;
    const void *replacee;
};

__attribute__((used))
static const struct quil_interpose quil_interposers[]
    __attribute__((section("__DATA,__interpose"))) = {
        {(const void *)quil_connect, (const void *)connect},
        {(const void *)quil_sendto, (const void *)sendto},
        {(const void *)quil_getaddrinfo, (const void *)getaddrinfo},
};
