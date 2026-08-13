#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <unistd.h>

/* 不依赖 Nano-X 的 UNIX98 PTY 冒烟程序。 */
int main(void) {
    int master = posix_openpt(O_RDWR | O_NOCTTY | O_NONBLOCK);
    if (master < 0) { perror("posix_openpt"); return 1; }
    if (grantpt(master) < 0 || unlockpt(master) < 0) {
        perror("grantpt/unlockpt"); return 1;
    }
    char slave_name[64];
    if (ptsname_r(master, slave_name, sizeof(slave_name)) != 0) {
        perror("ptsname_r"); return 1;
    }
    pid_t child = fork();
    if (child < 0) { perror("fork"); return 1; }
    if (child == 0) {
        if (setsid() < 0) _exit(120);
        int slave = open(slave_name, O_RDWR);
        if (slave < 0) _exit(121);
        if (dup2(slave, STDIN_FILENO) < 0 || dup2(slave, STDOUT_FILENO) < 0 ||
            dup2(slave, STDERR_FILENO) < 0) _exit(122);
        if (slave > STDERR_FILENO) close(slave);
        execl("/bin/sh", "sh", "-i", (char *)0);
        _exit(127);
    }

    static const char commands[] =
        "printf 'pty-ok\\n'; tty; stty size\n"
        "sleep 30\n"
        "printf 'ctrl-c-ok\\n'\n"
        "exit\n";
    if (write(master, commands, sizeof(commands) - 1) != (ssize_t)(sizeof(commands) - 1)) {
        perror("write commands"); return 1;
    }

    char output[4096];
    size_t used = 0;
    output[0] = '\0';
    int sent_interrupt = 0;
    int status = 0;
    int child_done = 0;
    for (int round = 0; round < 100 && !child_done; ++round) {
        struct pollfd pfd = { .fd = master, .events = POLLIN | POLLHUP };
        int ready = poll(&pfd, 1, 100);
        if (ready < 0 && errno == EINTR) continue;
        if (ready < 0) { perror("poll"); return 1; }
        if (ready > 0 && used + 1 < sizeof(output)) {
            ssize_t got = read(master, output + used, sizeof(output) - 1 - used);
            if (got > 0) {
                used += (size_t)got;
                output[used] = '\0';
            } else if (got < 0 && errno != EAGAIN && errno != EINTR && errno != EIO) {
                perror("read master"); return 1;
            }
        }
        if (!sent_interrupt && strstr(output, "pty-ok")) {
            usleep(200000);
            unsigned char intr = 3;
            if (write(master, &intr, 1) != 1) { perror("write VINTR"); return 1; }
            sent_interrupt = 1;
        }
        pid_t waited = waitpid(child, &status, WNOHANG);
        if (waited == child) child_done = 1;
        else if (waited < 0) { perror("waitpid"); return 1; }
    }
    output[used] = '\0';
    fputs(output, stdout);
    if (!child_done) {
        kill(child, SIGKILL);
        waitpid(child, &status, 0);
    }
    close(master);
    if (!strstr(output, "pty-ok") || !strstr(output, "/dev/pts/") ||
        !strstr(output, "ctrl-c-ok") || !sent_interrupt ||
        !WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        fprintf(stderr, "pty-smoke: failed child_status=%d\n", status);
        return 1;
    }
    puts("pty-smoke: success");
    return 0;
}
