#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <stdint.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#ifndef __NR_ioprio_set
#define __NR_ioprio_set 30
#define __NR_ioprio_get 31
#endif
#ifndef __NR_splice
#define __NR_splice 76
#endif
#ifndef __NR_sendfile
#define __NR_sendfile 71
#endif
#ifndef __NR_vmsplice
#define __NR_vmsplice 75
#define __NR_tee 77
#endif
#ifndef __NR_copy_file_range
#define __NR_copy_file_range 285
#endif
#ifndef __NR_timerfd_create
#define __NR_timerfd_create 85
#define __NR_timerfd_settime 86
#define __NR_timerfd_gettime 87
#endif
#ifndef __NR_recvmmsg
#define __NR_recvmmsg 243
#endif
#ifndef __NR_signalfd4
#define __NR_signalfd4 74
#endif

#define IOPRIO_WHO_PROCESS 1
#define IOPRIO_CLASS_BE 2
#define IOPRIO_PRIO_VALUE(class_, data_) (((class_) << 13) | (data_))
#define TFD_TIMER_ABSTIME 1
#define TFD_NONBLOCK 04000
#define TFD_CLOEXEC 02000000
#define SFD_NONBLOCK 04000
#define SFD_CLOEXEC 02000000
#ifndef MSG_WAITFORONE
#define MSG_WAITFORONE 0x10000
#endif

static void fail(const char *step) {
    fprintf(stderr, "[FAIL] %s: errno=%d (%s)\n", step, errno, strerror(errno));
    exit(1);
}

static void require(int condition, const char *step) {
    if (!condition) {
        errno = EIO;
        fail(step);
    }
}

static void read_exact_at(int fd, char *buffer, size_t length, off_t offset) {
    ssize_t count = pread(fd, buffer, length, offset);
    if (count < 0)
        fail("pread verification");
    require((size_t)count == length, "short verification read");
}

struct wos_signalfd_siginfo {
    uint32_t signo;
    int32_t error;
    int32_t code;
    uint32_t pid;
    uint32_t uid;
    uint8_t rest[108];
};

_Static_assert(sizeof(struct wos_signalfd_siginfo) == 128,
               "signalfd_siginfo ABI size");

int main(void) {
    static const char source_data[] = "0123456789abcdef";
    static const char initial_output[] = "................";
    const char *source_path = "/tmp/wos-cfr-source";
    const char *output_path = "/tmp/wos-cfr-output";
    int source = open(source_path, O_CREAT | O_TRUNC | O_RDWR, 0600);
    int output = open(output_path, O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (source < 0 || output < 0)
        fail("open transfer files");
    if (write(source, source_data, sizeof(source_data) - 1) != (ssize_t)(sizeof(source_data) - 1) ||
        write(output, initial_output, sizeof(initial_output) - 1) !=
            (ssize_t)(sizeof(initial_output) - 1))
        fail("seed transfer files");

    off_t input_offset = 2;
    off_t output_offset = 1;
    long copied = syscall(__NR_copy_file_range, source, &input_offset,
                          output, &output_offset, 5, 0);
    if (copied < 0)
        fail("copy_file_range");
    require(copied == 5 && input_offset == 7 && output_offset == 6,
            "copy_file_range offsets");
    char check[17] = {0};
    read_exact_at(output, check, 16, 0);
    require(memcmp(check, ".23456..........", 16) == 0,
            "copy_file_range content");

    errno = 0;
    require(syscall(__NR_copy_file_range, source, &input_offset,
                    output, &output_offset, 1, 1) == -1 && errno == EINVAL,
            "copy_file_range rejects flags");

    off_t sendfile_offset = 10;
    if (lseek(source, 3, SEEK_SET) != 3 || lseek(output, 0, SEEK_SET) != 0)
        fail("prepare positional sendfile");
    require(syscall(__NR_sendfile, output, source, &sendfile_offset, 3) == 3,
            "positional sendfile");
    require(sendfile_offset == 13 && lseek(source, 0, SEEK_CUR) == 3,
            "positional sendfile offset isolation");
    if (lseek(source, 4, SEEK_SET) != 4 || lseek(output, 4, SEEK_SET) != 4)
        fail("prepare sequential sendfile");
    require(syscall(__NR_sendfile, output, source, NULL, 2) == 2,
            "sequential sendfile");
    require(lseek(source, 0, SEEK_CUR) == 6 && lseek(output, 0, SEEK_CUR) == 6,
            "sequential sendfile positions");

    int pipefd[2];
    if (pipe(pipefd) < 0)
        fail("pipe");
    if (lseek(source, 0, SEEK_SET) != 0)
        fail("rewind source");
    long moved = syscall(__NR_splice, source, NULL, pipefd[1], NULL, 6, 0);
    if (moved < 0)
        fail("splice file to pipe");
    require(moved == 6 && lseek(source, 0, SEEK_CUR) == 6,
            "splice input position");
    off_t splice_output = 8;
    moved = syscall(__NR_splice, pipefd[0], NULL, output, &splice_output, 6, 0);
    if (moved < 0)
        fail("splice pipe to file");
    require(moved == 6 && splice_output == 14, "splice output position");
    read_exact_at(output, check, 16, 0);
    require(memcmp(check + 8, "012345", 6) == 0, "splice content");

    errno = 0;
    require(syscall(__NR_splice, source, NULL, output, NULL, 1, 0) == -1 && errno == EINVAL,
            "splice requires a pipe");

    int tee_source[2], tee_output[2];
    if (pipe(tee_source) < 0 || pipe(tee_output) < 0)
        fail("tee pipes");
    if (write(tee_source[1], "abcdef", 6) != 6)
        fail("seed tee pipe");
    require(syscall(__NR_tee, tee_source[0], tee_output[1], 3, 0) == 3,
            "tee copy");
    char pipe_check[8] = {0};
    require(read(tee_output[0], pipe_check, 3) == 3 && memcmp(pipe_check, "abc", 3) == 0,
            "tee output");
    memset(pipe_check, 0, sizeof(pipe_check));
    require(read(tee_source[0], pipe_check, 6) == 6 && memcmp(pipe_check, "abcdef", 6) == 0,
            "tee preserves input");

    int vm_pipe[2];
    if (pipe(vm_pipe) < 0)
        fail("vmsplice pipe");
    struct iovec vectors[2] = {
        {.iov_base = (void *)"xy", .iov_len = 2},
        {.iov_base = (void *)"z12", .iov_len = 3},
    };
    require(syscall(__NR_vmsplice, vm_pipe[1], vectors, 2, 0) == 5,
            "vmsplice write");
    memset(pipe_check, 0, sizeof(pipe_check));
    require(read(vm_pipe[0], pipe_check, 5) == 5 && memcmp(pipe_check, "xyz12", 5) == 0,
            "vmsplice content");

    int priority = IOPRIO_PRIO_VALUE(IOPRIO_CLASS_BE, 5);
    if (syscall(__NR_ioprio_set, IOPRIO_WHO_PROCESS, 0, priority) < 0)
        fail("ioprio_set");
    require(syscall(__NR_ioprio_get, IOPRIO_WHO_PROCESS, 0) == priority,
            "ioprio_get");
    pid_t child = fork();
    if (child < 0)
        fail("fork ioprio inheritance");
    if (child == 0)
        _exit(syscall(__NR_ioprio_get, IOPRIO_WHO_PROCESS, 0) == priority ? 0 : 1);
    int status = 0;
    if (waitpid(child, &status, 0) != child)
        fail("waitpid ioprio inheritance");
    require(WIFEXITED(status) && WEXITSTATUS(status) == 0, "ioprio fork inheritance");

    int timerfd = syscall(__NR_timerfd_create, CLOCK_MONOTONIC,
                          TFD_NONBLOCK | TFD_CLOEXEC);
    if (timerfd < 0)
        fail("timerfd_create");
    require((fcntl(timerfd, F_GETFD) & FD_CLOEXEC) != 0,
            "timerfd cloexec");
    uint64_t expirations = 0;
    errno = 0;
    require(read(timerfd, &expirations, sizeof(expirations)) == -1 && errno == EAGAIN,
            "unarmed timerfd is not readable");

    struct itimerspec invalid_timer = {
        .it_value = {.tv_sec = 0, .tv_nsec = 1000000000L},
    };
    errno = 0;
    require(syscall(__NR_timerfd_settime, timerfd, 0, &invalid_timer, NULL) == -1 &&
            errno == EINVAL, "timerfd rejects invalid timespec");

    struct itimerspec timer = {
        .it_interval = {.tv_sec = 0, .tv_nsec = 0},
        .it_value = {.tv_sec = 0, .tv_nsec = 30000000L},
    };
    struct itimerspec old_timer = {{0}};
    require(syscall(__NR_timerfd_settime, timerfd, 0, &timer, &old_timer) == 0,
            "arm one-shot timerfd");
    require(old_timer.it_value.tv_sec == 0 && old_timer.it_value.tv_nsec == 0,
            "new timerfd old value is disarmed");
    struct itimerspec current_timer = {{0}};
    require(syscall(__NR_timerfd_gettime, timerfd, &current_timer) == 0,
            "timerfd_gettime");
    require(current_timer.it_value.tv_sec == 0 && current_timer.it_value.tv_nsec > 0 &&
            current_timer.it_value.tv_nsec <= 30000000L,
            "timerfd remaining time");
    struct pollfd timer_poll = {.fd = timerfd, .events = POLLIN};
    require(poll(&timer_poll, 1, 500) == 1 && (timer_poll.revents & POLLIN) != 0,
            "poll timerfd expiration");
    require(read(timerfd, &expirations, sizeof(expirations)) == sizeof(expirations) &&
            expirations == 1, "read one-shot timerfd");

    timer.it_interval.tv_nsec = 10000000L;
    timer.it_value.tv_nsec = 10000000L;
    require(syscall(__NR_timerfd_settime, timerfd, 0, &timer, NULL) == 0,
            "arm periodic timerfd");
    struct timespec pause = {.tv_sec = 0, .tv_nsec = 55000000L};
    if (nanosleep(&pause, NULL) < 0)
        fail("timerfd accumulation sleep");
    require(read(timerfd, &expirations, sizeof(expirations)) == sizeof(expirations) &&
            expirations >= 3, "timerfd accumulates overruns");

    int duplicated_timerfd = dup(timerfd);
    if (duplicated_timerfd < 0)
        fail("dup timerfd");
    timer.it_interval.tv_nsec = 0;
    timer.it_value.tv_nsec = 20000000L;
    require(syscall(__NR_timerfd_settime, duplicated_timerfd, 0, &timer, NULL) == 0,
            "arm duplicated timerfd");
    timer_poll.fd = timerfd;
    timer_poll.revents = 0;
    require(poll(&timer_poll, 1, 500) == 1, "shared timerfd poll");
    require(read(timerfd, &expirations, sizeof(expirations)) == sizeof(expirations),
            "shared timerfd read");
    errno = 0;
    require(read(duplicated_timerfd, &expirations, sizeof(expirations)) == -1 && errno == EAGAIN,
            "timerfd dup shares counter");

    int receive_socket = socket(AF_INET, SOCK_DGRAM, 0);
    int send_socket = socket(AF_INET, SOCK_DGRAM, 0);
    if (receive_socket < 0 || send_socket < 0)
        fail("recvmmsg sockets");
    struct sockaddr_in receive_address = {
        .sin_family = AF_INET,
        .sin_port = 0,
        .sin_addr.s_addr = htonl(INADDR_LOOPBACK),
    };
    if (bind(receive_socket, (struct sockaddr *)&receive_address,
             sizeof(receive_address)) < 0)
        fail("recvmmsg bind");
    socklen_t receive_address_len = sizeof(receive_address);
    if (getsockname(receive_socket, (struct sockaddr *)&receive_address,
                    &receive_address_len) < 0)
        fail("recvmmsg getsockname");
    if (sendto(send_socket, "one", 3, 0, (struct sockaddr *)&receive_address,
               sizeof(receive_address)) != 3 ||
        sendto(send_socket, "two", 3, 0, (struct sockaddr *)&receive_address,
               sizeof(receive_address)) != 3)
        fail("seed recvmmsg datagrams");

    char receive_buffers[2][8] = {{0}};
    struct iovec receive_iov[2] = {
        {.iov_base = receive_buffers[0], .iov_len = sizeof(receive_buffers[0])},
        {.iov_base = receive_buffers[1], .iov_len = sizeof(receive_buffers[1])},
    };
    struct mmsghdr messages[2];
    memset(messages, 0, sizeof(messages));
    messages[0].msg_hdr.msg_iov = &receive_iov[0];
    messages[0].msg_hdr.msg_iovlen = 1;
    messages[1].msg_hdr.msg_iov = &receive_iov[1];
    messages[1].msg_hdr.msg_iovlen = 1;
    struct timespec receive_timeout = {.tv_sec = 1, .tv_nsec = 0};
    long received_messages = syscall(__NR_recvmmsg, receive_socket, messages, 2, 0,
                                     &receive_timeout);
    if (received_messages < 0)
        fail("recvmmsg datagrams");
    require(received_messages == 2 && messages[0].msg_len == 3 &&
            messages[1].msg_len == 3 && memcmp(receive_buffers[0], "one", 3) == 0 &&
            memcmp(receive_buffers[1], "two", 3) == 0,
            "recvmmsg data and lengths");

    receive_timeout.tv_sec = 0;
    receive_timeout.tv_nsec = 30000000L;
    memset(messages, 0, sizeof(messages));
    messages[0].msg_hdr.msg_iov = &receive_iov[0];
    messages[0].msg_hdr.msg_iovlen = 1;
    require(syscall(__NR_recvmmsg, receive_socket, messages, 1, 0,
                    &receive_timeout) == 0,
            "recvmmsg timeout with no data");

    sigset_t signal_mask;
    sigemptyset(&signal_mask);
    sigaddset(&signal_mask, SIGUSR1);
    sigaddset(&signal_mask, SIGUSR2);
    if (sigprocmask(SIG_BLOCK, &signal_mask, NULL) < 0)
        fail("block signalfd signals");
    uint64_t signal_mask_bits = 0;
    memcpy(&signal_mask_bits, &signal_mask, sizeof(signal_mask_bits));
    int signal_fd = syscall(__NR_signalfd4, -1, &signal_mask_bits,
                            sizeof(signal_mask_bits), SFD_NONBLOCK | SFD_CLOEXEC);
    if (signal_fd < 0)
        fail("signalfd4 create");
    require((fcntl(signal_fd, F_GETFD) & FD_CLOEXEC) != 0,
            "signalfd cloexec");
    struct wos_signalfd_siginfo signal_info[2];
    errno = 0;
    require(read(signal_fd, signal_info, sizeof(signal_info[0])) == -1 && errno == EAGAIN,
            "empty signalfd is nonblocking");
    if (kill(getpid(), SIGUSR1) < 0 || kill(getpid(), SIGUSR2) < 0)
        fail("queue signalfd signals");
    struct pollfd signal_poll = {.fd = signal_fd, .events = POLLIN};
    require(poll(&signal_poll, 1, 100) == 1 && (signal_poll.revents & POLLIN) != 0,
            "poll signalfd pending");
    volatile uintptr_t invalid_user_address = 1;
    errno = 0;
    require(syscall(__NR_read, signal_fd, (void *)invalid_user_address,
                    sizeof(signal_info[0])) == -1 && errno == EFAULT,
            "signalfd user fault");
    memset(signal_info, 0, sizeof(signal_info));
    require(read(signal_fd, signal_info, sizeof(signal_info)) == sizeof(signal_info),
            "signalfd batch read after rollback");
    require(signal_info[0].signo == SIGUSR1 && signal_info[1].signo == SIGUSR2 &&
            signal_info[0].pid == (uint32_t)getpid(),
            "signalfd records and source");

    sigemptyset(&signal_mask);
    sigaddset(&signal_mask, SIGUSR2);
    signal_mask_bits = 0;
    memcpy(&signal_mask_bits, &signal_mask, sizeof(signal_mask_bits));
    require(syscall(__NR_signalfd4, signal_fd, &signal_mask_bits,
                    sizeof(signal_mask_bits), 0) == signal_fd,
            "signalfd mask update");
    if (kill(getpid(), SIGUSR2) < 0)
        fail("queue updated signalfd signal");
    require(read(signal_fd, signal_info, sizeof(signal_info[0])) == sizeof(signal_info[0]) &&
            signal_info[0].signo == SIGUSR2,
            "updated signalfd mask is active");
    sigaddset(&signal_mask, SIGUSR1);
    if (sigprocmask(SIG_UNBLOCK, &signal_mask, NULL) < 0)
        fail("unblock signalfd signals");

    close(pipefd[0]);
    close(pipefd[1]);
    close(source);
    close(output);
    close(tee_source[0]);
    close(tee_source[1]);
    close(tee_output[0]);
    close(tee_output[1]);
    close(vm_pipe[0]);
    close(vm_pipe[1]);
    close(duplicated_timerfd);
    close(timerfd);
    close(receive_socket);
    close(send_socket);
    close(signal_fd);
    unlink(source_path);
    unlink(output_path);
    puts("[PASS] copy_file_range: content, offsets, flags");
    puts("[PASS] sendfile: positional isolation and sequential lease progress");
    puts("[PASS] splice: file->pipe->file, positions, validation");
    puts("[PASS] tee/vmsplice: duplicate without consume and iovec input");
    puts("[PASS] ioprio: set/get and fork inheritance");
    puts("[PASS] timerfd: nonblock, gettime, poll, periodic accumulation, dup sharing");
    puts("[PASS] recvmmsg: UDP batch data, lengths and timeout");
    puts("[PASS] signalfd: mask, poll, batch read, EFAULT rollback, source and update");
    return 0;
}
