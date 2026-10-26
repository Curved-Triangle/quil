#include <arpa/inet.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) {
        return 2;
    }
    struct sockaddr_in address = {0};
    address.sin_family = AF_INET;
    address.sin_port = htons(9);
    (void)inet_pton(AF_INET, "127.0.0.1", &address.sin_addr);
    /* The test interposer and seatbelt profile both prevent an actual connection. */
    (void)connect(fd, (struct sockaddr *)&address, sizeof(address));
    (void)close(fd);
    return 0;
}
