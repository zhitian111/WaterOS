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
#include <sys/mman.h>
#include <sys/msg.h>
#include <sys/sem.h>
#include <sys/socket.h>
#include <sys/stat.h>
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
#ifndef __NR_memfd_create
#define __NR_memfd_create 279
#endif
#ifndef __NR_inotify_init1
#define __NR_inotify_init1 26
#define __NR_inotify_add_watch 27
#define __NR_inotify_rm_watch 28
#endif
#ifndef __NR_openat2
#define __NR_openat2 437
#endif
#ifndef __NR_futex
#define __NR_futex 98
#endif
#ifndef __NR_pidfd_send_signal
#define __NR_pidfd_send_signal 424
#define __NR_pidfd_open 434
#define __NR_pidfd_getfd 438
#endif
#ifndef __NR_clone3
#define __NR_clone3 435
#endif

#define IOPRIO_WHO_PROCESS 1
#define IOPRIO_CLASS_BE 2
#define IOPRIO_PRIO_VALUE(class_, data_) (((class_) << 13) | (data_))
#define TFD_TIMER_ABSTIME 1
#define TFD_NONBLOCK 04000
#define TFD_CLOEXEC 02000000
#define SFD_NONBLOCK 04000
#define SFD_CLOEXEC 02000000
#ifndef MFD_CLOEXEC
#define MFD_CLOEXEC 0x0001U
#define MFD_ALLOW_SEALING 0x0002U
#endif
#ifndef F_ADD_SEALS
#define F_ADD_SEALS 1033
#define F_GET_SEALS 1034
#define F_SEAL_SEAL 0x0001
#define F_SEAL_SHRINK 0x0002
#define F_SEAL_GROW 0x0004
#define F_SEAL_WRITE 0x0008
#endif
#ifndef MSG_WAITFORONE
#define MSG_WAITFORONE 0x10000
#endif
#ifndef MADV_POPULATE_WRITE
#define MADV_POPULATE_WRITE 23
#endif

/* glibc/musl 都可能要求调用方自行声明 semctl 的 union。 */
union wos_semun {
    int val;
    struct semid_ds *buf;
    unsigned short *array;
};

#define IN_ACCESS       0x00000001U
#define IN_MODIFY       0x00000002U
#define IN_ATTRIB       0x00000004U
#define IN_MOVED_FROM   0x00000040U
#define IN_MOVED_TO     0x00000080U
#define IN_CREATE       0x00000100U
#define IN_DELETE       0x00000200U
#define IN_DELETE_SELF  0x00000400U
#define IN_IGNORED      0x00008000U
#define IN_NONBLOCK     00004000
#define IN_CLOEXEC      02000000

struct wos_inotify_event {
    int32_t wd;
    uint32_t mask;
    uint32_t cookie;
    uint32_t len;
    char name[];
};

struct wos_open_how {
    uint64_t flags;
    uint64_t mode;
    uint64_t resolve;
};

#define RESOLVE_NO_SYMLINKS 0x04U
#define RESOLVE_BENEATH     0x08U
#define RESOLVE_CACHED      0x20U
#define FUTEX_WAIT_BITSET   9
#define FUTEX_WAKE_BITSET   10
#define FUTEX_CMP_REQUEUE   4
#define FUTEX_WAIT          0
#define FUTEX_WAKE          1
#define FUTEX_WAKE_OP       5
#define FUTEX_OP_ADD        1
#define FUTEX_OP_CMP_EQ     0
#define FUTEX_OP_CMP_NE     1
#define FUTEX_OP(op, oparg, cmp, cmparg) \
    ((((op) & 0xfU) << 28) | (((cmp) & 0xfU) << 24) | \
     (((oparg) & 0xfffU) << 12) | ((cmparg) & 0xfffU))
#define CLONE_PIDFD         0x00001000ULL

struct wos_clone_args {
    uint64_t flags;
    uint64_t pidfd;
    uint64_t child_tid;
    uint64_t parent_tid;
    uint64_t exit_signal;
    uint64_t stack;
    uint64_t stack_size;
    uint64_t tls;
    uint64_t set_tid;
    uint64_t set_tid_size;
    uint64_t cgroup;
};

struct wos_msg_buf {
    long mtype;
    char mtext[64];
};

_Static_assert(sizeof(struct wos_inotify_event) == 16,
               "inotify_event ABI size");

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

    int memfd = syscall(__NR_memfd_create, "wateros-smoke",
                        MFD_CLOEXEC | MFD_ALLOW_SEALING);
    if (memfd < 0)
        fail("memfd_create");
    require((fcntl(memfd, F_GETFD) & FD_CLOEXEC) != 0,
            "memfd cloexec");
    require(fcntl(memfd, F_GET_SEALS) == 0, "memfd initial seals");
    require(ftruncate(memfd, 4096) == 0, "memfd grow");
    require(pwrite(memfd, "memfd", 5, 0) == 5, "memfd pwrite");
    char *memfd_map = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, memfd, 0);
    require(memfd_map != MAP_FAILED && memcmp(memfd_map, "memfd", 5) == 0,
            "memfd shared mmap");
    memcpy(memfd_map, "MAP", 3);
    require(msync(memfd_map, 4096, MS_SYNC) == 0, "memfd msync");
    char memfd_verify[6] = {0};
    require(pread(memfd, memfd_verify, 5, 0) == 5 &&
            memcmp(memfd_verify, "MAPfd", 5) == 0,
            "memfd shared writeback");
    errno = 0;
    require(fcntl(memfd, F_ADD_SEALS, F_SEAL_WRITE) == -1 && errno == EBUSY,
            "memfd rejects write seal while writable mapped");
    require(munmap(memfd_map, 4096) == 0, "memfd munmap");
    require(fcntl(memfd, F_ADD_SEALS, F_SEAL_SHRINK | F_SEAL_GROW) == 0,
            "memfd size seals");
    errno = 0;
    require(ftruncate(memfd, 2048) == -1 && errno == EPERM,
            "memfd shrink seal");
    errno = 0;
    require(ftruncate(memfd, 8192) == -1 && errno == EPERM,
            "memfd grow seal");
    require(fcntl(memfd, F_ADD_SEALS, F_SEAL_WRITE) == 0,
            "memfd write seal");
    errno = 0;
    require(pwrite(memfd, "x", 1, 0) == -1 && errno == EPERM,
            "memfd sealed write");
    require(fcntl(memfd, F_GET_SEALS) ==
            (F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE),
            "memfd seal query");

    int fixed_seal_memfd = syscall(__NR_memfd_create, "wateros-fixed", 0);
    if (fixed_seal_memfd < 0)
        fail("memfd_create no sealing");
    require(fcntl(fixed_seal_memfd, F_GET_SEALS) == F_SEAL_SEAL,
            "memfd no-allow-sealing starts sealed");
    errno = 0;
    require(fcntl(fixed_seal_memfd, F_ADD_SEALS, F_SEAL_GROW) == -1 && errno == EPERM,
            "memfd seal-seal blocks additions");

    const char *notify_dir = "/tmp/wos-inotify";
    const char *notify_old = "/tmp/wos-inotify/old";
    const char *notify_new = "/tmp/wos-inotify/new";
    unlink(notify_old);
    unlink(notify_new);
    rmdir(notify_dir);
    require(mkdir(notify_dir, 0700) == 0, "inotify test mkdir");
    int notify_fd = syscall(__NR_inotify_init1, IN_NONBLOCK | IN_CLOEXEC);
    if (notify_fd < 0)
        fail("inotify_init1");
    require((fcntl(notify_fd, F_GETFD) & FD_CLOEXEC) != 0,
            "inotify cloexec");
    int dir_wd = syscall(__NR_inotify_add_watch, notify_fd, notify_dir,
                         IN_CREATE | IN_MODIFY | IN_MOVED_FROM | IN_MOVED_TO | IN_DELETE);
    if (dir_wd < 0)
        fail("inotify_add_watch directory");
    int notify_file = open(notify_old, O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (notify_file < 0 || write(notify_file, "event", 5) != 5)
        fail("create inotify file");
    int file_wd = syscall(__NR_inotify_add_watch, notify_fd, notify_old,
                          IN_ATTRIB | IN_DELETE_SELF);
    if (file_wd < 0)
        fail("inotify_add_watch file");
    require(chmod(notify_old, 0640) == 0, "inotify chmod");
    require(rename(notify_old, notify_new) == 0, "inotify rename");
    require(unlink(notify_new) == 0, "inotify unlink");

    struct pollfd notify_poll = {.fd = notify_fd, .events = POLLIN};
    require(poll(&notify_poll, 1, 100) == 1 && (notify_poll.revents & POLLIN) != 0,
            "inotify poll");
    unsigned char notify_events[2048];
    ssize_t notify_bytes = read(notify_fd, notify_events, sizeof(notify_events));
    if (notify_bytes < 0)
        fail("inotify event read");
    int saw_create = 0, saw_modify = 0, saw_from = 0, saw_to = 0, saw_delete = 0;
    int saw_attrib = 0, saw_delete_self = 0, saw_ignored = 0;
    uint32_t from_cookie = 0, to_cookie = 0;
    for (size_t offset = 0; offset + sizeof(struct wos_inotify_event) <= (size_t)notify_bytes;) {
        struct wos_inotify_event *event =
            (struct wos_inotify_event *)(notify_events + offset);
        size_t event_size = sizeof(*event) + event->len;
        require(event_size >= sizeof(*event) && offset + event_size <= (size_t)notify_bytes,
                "inotify event bounds");
        if (event->wd == dir_wd && event->len != 0 && strcmp(event->name, "old") == 0) {
            saw_create |= (event->mask & IN_CREATE) != 0;
            saw_modify |= (event->mask & IN_MODIFY) != 0;
            if (event->mask & IN_MOVED_FROM) {
                saw_from = 1;
                from_cookie = event->cookie;
            }
        }
        if (event->wd == dir_wd && event->len != 0 && strcmp(event->name, "new") == 0) {
            if (event->mask & IN_MOVED_TO) {
                saw_to = 1;
                to_cookie = event->cookie;
            }
            saw_delete |= (event->mask & IN_DELETE) != 0;
        }
        if (event->wd == file_wd) {
            saw_attrib |= (event->mask & IN_ATTRIB) != 0;
            saw_delete_self |= (event->mask & IN_DELETE_SELF) != 0;
            saw_ignored |= (event->mask & IN_IGNORED) != 0;
        }
        offset += event_size;
    }
    require(saw_create && saw_modify && saw_from && saw_to && saw_delete,
            "inotify directory event set");
    require(from_cookie != 0 && from_cookie == to_cookie,
            "inotify rename cookie");
    require(saw_attrib && saw_delete_self && saw_ignored,
            "inotify watched-file lifecycle");

    notify_file = open(notify_old, O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (notify_file < 0)
        fail("inotify rollback seed");
    errno = 0;
    require(syscall(__NR_read, notify_fd, (void *)invalid_user_address,
                    sizeof(notify_events)) == -1 && errno == EFAULT,
            "inotify user fault");
    notify_bytes = read(notify_fd, notify_events, sizeof(notify_events));
    require(notify_bytes >= (ssize_t)sizeof(struct wos_inotify_event),
            "inotify EFAULT rollback");
    require(syscall(__NR_inotify_rm_watch, notify_fd, dir_wd) == 0,
            "inotify_rm_watch");

    const char *openat2_dir = "/tmp/wos-openat2";
    const char *openat2_file = "/tmp/wos-openat2/file";
    const char *openat2_link = "/tmp/wos-openat2/link";
    unlink(openat2_link);
    unlink(openat2_file);
    rmdir(openat2_dir);
    require(mkdir(openat2_dir, 0700) == 0, "openat2 test mkdir");
    int openat2_dirfd = open(openat2_dir, O_RDONLY | O_DIRECTORY);
    if (openat2_dirfd < 0)
        fail("openat2 directory fd");
    struct wos_open_how how = {.flags = O_CREAT | O_RDWR | O_CLOEXEC, .mode = 0600};
    int openat2_fd = syscall(__NR_openat2, openat2_dirfd, "file", &how, sizeof(how));
    if (openat2_fd < 0)
        fail("openat2 create");
    require((fcntl(openat2_fd, F_GETFD) & FD_CLOEXEC) != 0 &&
            write(openat2_fd, "openat2", 7) == 7,
            "openat2 flags and I/O");
    require(symlink("file", openat2_link) == 0, "openat2 symlink seed");
    how.flags = O_RDONLY;
    how.mode = 0;
    how.resolve = RESOLVE_NO_SYMLINKS;
    errno = 0;
    require(syscall(__NR_openat2, openat2_dirfd, "link", &how, sizeof(how)) == -1 &&
            errno == ELOOP, "openat2 no symlinks");
    how.resolve = RESOLVE_BENEATH;
    errno = 0;
    require(syscall(__NR_openat2, openat2_dirfd, "../escape", &how, sizeof(how)) == -1 &&
            errno == EXDEV, "openat2 beneath escape");
    how.resolve = RESOLVE_CACHED;
    errno = 0;
    require(syscall(__NR_openat2, openat2_dirfd, "file", &how, sizeof(how)) == -1 &&
            errno == EAGAIN, "openat2 cached fallback");
    struct {
        struct wos_open_how how;
        uint64_t extension;
    } extended_how = {.how = {.flags = O_RDONLY}};
    int extended_fd = syscall(__NR_openat2, openat2_dirfd, "file", &extended_how,
                              sizeof(extended_how));
    if (extended_fd < 0)
        fail("openat2 zero extension");
    close(extended_fd);
    extended_how.extension = 1;
    errno = 0;
    require(syscall(__NR_openat2, openat2_dirfd, "file", &extended_how,
                    sizeof(extended_how)) == -1 && errno == E2BIG,
            "openat2 rejects nonzero extension");

    int futex_memfd = syscall(__NR_memfd_create, "futex-bitset", 0);
    if (futex_memfd < 0 || ftruncate(futex_memfd, 4096) < 0)
        fail("futex bitset backing");
    uint32_t *futex_words = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                                 MAP_SHARED, futex_memfd, 0);
    require(futex_words != MAP_FAILED, "futex bitset shared map");
    futex_words[0] = 0;
    futex_words[1] = 0;
    pid_t bit_child_one = fork();
    if (bit_child_one < 0)
        fail("fork futex waiter one");
    if (bit_child_one == 0) {
        __atomic_fetch_add(&futex_words[1], 1, __ATOMIC_SEQ_CST);
        long result = syscall(__NR_futex, &futex_words[0], FUTEX_WAIT_BITSET,
                              0, NULL, NULL, 0x1U);
        _exit(result == 0 ? 0 : 1);
    }
    pid_t bit_child_two = fork();
    if (bit_child_two < 0)
        fail("fork futex waiter two");
    if (bit_child_two == 0) {
        __atomic_fetch_add(&futex_words[1], 1, __ATOMIC_SEQ_CST);
        long result = syscall(__NR_futex, &futex_words[0], FUTEX_WAIT_BITSET,
                              0, NULL, NULL, 0x2U);
        _exit(result == 0 ? 0 : 1);
    }
    for (int attempt = 0; attempt < 100 &&
                              __atomic_load_n(&futex_words[1], __ATOMIC_SEQ_CST) != 2;
         ++attempt) {
        struct timespec short_pause = {.tv_sec = 0, .tv_nsec = 1000000L};
        nanosleep(&short_pause, NULL);
    }
    require(__atomic_load_n(&futex_words[1], __ATOMIC_SEQ_CST) == 2,
            "futex waiters started");
    struct timespec futex_settle = {.tv_sec = 0, .tv_nsec = 30000000L};
    nanosleep(&futex_settle, NULL);
    require(syscall(__NR_futex, &futex_words[0], FUTEX_WAKE_BITSET,
                    1, NULL, NULL, 0x1U) == 1,
            "futex bitset selective wake one");
    require(waitpid(bit_child_one, &status, 0) == bit_child_one &&
            WIFEXITED(status) && WEXITSTATUS(status) == 0,
            "futex bitset first waiter result");
    require(waitpid(bit_child_two, &status, WNOHANG) == 0,
            "futex bitset leaves nonmatching waiter asleep");
    require(syscall(__NR_futex, &futex_words[0], FUTEX_WAKE_BITSET,
                    1, NULL, NULL, 0x2U) == 1,
            "futex bitset selective wake two");
    require(waitpid(bit_child_two, &status, 0) == bit_child_two &&
            WIFEXITED(status) && WEXITSTATUS(status) == 0,
            "futex bitset second waiter result");

    /* requeue 后 waiter 的 bitset 与生命周期登记必须跟随目标地址。 */
    futex_words[0] = 0;
    futex_words[1] = 0;
    futex_words[2] = 0;
    pid_t requeue_child = fork();
    if (requeue_child < 0)
        fail("fork futex requeue waiter");
    if (requeue_child == 0) {
        __atomic_store_n(&futex_words[2], 1, __ATOMIC_SEQ_CST);
        long result = syscall(__NR_futex, &futex_words[0], FUTEX_WAIT_BITSET,
                              0, NULL, NULL, 0x4U);
        _exit(result == 0 ? 0 : 1);
    }
    for (int attempt = 0; attempt < 100 &&
                              __atomic_load_n(&futex_words[2], __ATOMIC_SEQ_CST) != 1;
         ++attempt) {
        struct timespec short_pause = {.tv_sec = 0, .tv_nsec = 1000000L};
        nanosleep(&short_pause, NULL);
    }
    require(__atomic_load_n(&futex_words[2], __ATOMIC_SEQ_CST) == 1,
            "futex requeue waiter started");
    nanosleep(&futex_settle, NULL);
    require(syscall(__NR_futex, &futex_words[0], FUTEX_CMP_REQUEUE,
                    0, 1, &futex_words[1], 0) == 1,
            "futex cmp requeue one");
    require(waitpid(requeue_child, &status, WNOHANG) == 0,
            "futex requeue does not wake moved waiter");
    require(syscall(__NR_futex, &futex_words[1], FUTEX_WAKE_BITSET,
                    1, NULL, NULL, 0x4U) == 1,
            "futex wake bitset after requeue");
    require(waitpid(requeue_child, &status, 0) == requeue_child &&
            WIFEXITED(status) && WEXITSTATUS(status) == 0,
            "futex requeue waiter result");

    /* WAKE_OP 必须先原子更新第二个 word，再按更新前的值决定是否唤醒。 */
    futex_words[0] = 0;
    futex_words[1] = 7;
    futex_words[3] = 0;
    pid_t wake_op_child = fork();
    if (wake_op_child < 0)
        fail("fork futex wake-op waiter");
    if (wake_op_child == 0) {
        __atomic_store_n(&futex_words[3], 1, __ATOMIC_SEQ_CST);
        long result = syscall(__NR_futex, &futex_words[1], FUTEX_WAIT,
                              7, NULL, NULL, 0);
        _exit(result == 0 ? 0 : 1);
    }
    for (int attempt = 0; attempt < 100 &&
                              __atomic_load_n(&futex_words[3], __ATOMIC_SEQ_CST) != 1;
         ++attempt) {
        struct timespec short_pause = {.tv_sec = 0, .tv_nsec = 1000000L};
        nanosleep(&short_pause, NULL);
    }
    require(__atomic_load_n(&futex_words[3], __ATOMIC_SEQ_CST) == 1,
            "futex wake-op waiter started");
    nanosleep(&futex_settle, NULL);
    unsigned wake_op = FUTEX_OP(FUTEX_OP_ADD, 2, FUTEX_OP_CMP_EQ, 7);
    require(syscall(__NR_futex, &futex_words[0], FUTEX_WAKE_OP,
                    0, 1, &futex_words[1], wake_op) == 1 &&
            __atomic_load_n(&futex_words[1], __ATOMIC_SEQ_CST) == 9,
            "futex wake-op atomic update and conditional wake");
    require(waitpid(wake_op_child, &status, 0) == wake_op_child &&
            WIFEXITED(status) && WEXITSTATUS(status) == 0,
            "futex wake-op waiter result");

    int clone_pidfd = -1;
    struct wos_clone_args clone_args;
    memset(&clone_args, 0, sizeof(clone_args));
    clone_args.flags = CLONE_PIDFD;
    clone_args.pidfd = (uintptr_t)&clone_pidfd;
    clone_args.exit_signal = SIGCHLD;
    pid_t clone_pidfd_child = syscall(__NR_clone3, &clone_args, sizeof(clone_args));
    if (clone_pidfd_child < 0)
        fail("clone3 CLONE_PIDFD");
    if (clone_pidfd_child == 0)
        _exit(23);
    require(clone_pidfd >= 0 &&
            (fcntl(clone_pidfd, F_GETFD) & FD_CLOEXEC) != 0,
            "clone3 returned close-on-exec pidfd");
    struct pollfd clone_pidfd_poll = {.fd = clone_pidfd, .events = POLLIN};
    require(poll(&clone_pidfd_poll, 1, 5000) == 1 &&
            (clone_pidfd_poll.revents & (POLLIN | POLLHUP)) != 0,
            "clone pidfd poll exit readiness");
    siginfo_t clone_pidfd_info;
    memset(&clone_pidfd_info, 0, sizeof(clone_pidfd_info));
    require(syscall(__NR_waitid, 3, clone_pidfd, &clone_pidfd_info,
                    WEXITED, NULL) == 0 &&
            clone_pidfd_info.si_pid == clone_pidfd_child &&
            clone_pidfd_info.si_status == 23,
            "clone pidfd waitid exit status");

    /* WAKE_OP：条件不满足时仍原子更新第二个 word，但跳过第二队列唤醒。 */
    futex_words[0] = 0;
    futex_words[1] = 7;
    futex_words[4] = 0;
    pid_t wake_op_miss_child = fork();
    if (wake_op_miss_child < 0)
        fail("fork futex wake-op miss waiter");
    if (wake_op_miss_child == 0) {
        __atomic_store_n(&futex_words[4], 1, __ATOMIC_SEQ_CST);
        long result = syscall(__NR_futex, &futex_words[1], FUTEX_WAIT,
                              7, NULL, NULL, 0);
        _exit(result == 0 ? 0 : 1);
    }
    for (int attempt = 0; attempt < 100 &&
                              __atomic_load_n(&futex_words[4], __ATOMIC_SEQ_CST) != 1;
         ++attempt) {
        struct timespec short_pause = {.tv_sec = 0, .tv_nsec = 1000000L};
        nanosleep(&short_pause, NULL);
    }
    require(__atomic_load_n(&futex_words[4], __ATOMIC_SEQ_CST) == 1,
            "futex wake-op miss waiter started");
    nanosleep(&futex_settle, NULL);
    unsigned wake_op_miss = FUTEX_OP(FUTEX_OP_ADD, 2, FUTEX_OP_CMP_NE, 7);
    require(syscall(__NR_futex, &futex_words[0], FUTEX_WAKE_OP,
                    0, 1, &futex_words[1], wake_op_miss) == 0 &&
            __atomic_load_n(&futex_words[1], __ATOMIC_SEQ_CST) == 9,
            "futex wake-op updates word but skips second wake on miss");
    require(waitpid(wake_op_miss_child, &status, WNOHANG) == 0,
            "futex wake-op miss leaves waiter asleep");
    require(syscall(__NR_futex, &futex_words[1], FUTEX_WAKE, 1, NULL, NULL, 0) == 1,
            "futex wake-op miss waiter manual wake");
    require(waitpid(wake_op_miss_child, &status, 0) == wake_op_miss_child &&
            WIFEXITED(status) && WEXITSTATUS(status) == 0,
            "futex wake-op miss waiter result");

    errno = 0;
    require(syscall(__NR_futex, &futex_words[0], FUTEX_WAKE_OP,
                    0, 1, &futex_words[1], FUTEX_OP(5, 0, FUTEX_OP_CMP_EQ, 0)) == -1 &&
            errno == ENOSYS,
            "futex wake-op rejects unsupported operation");

    /* clone3 CLONE_PIDFD：pidfd 指针为空时应在启动子进程前失败。 */
    struct wos_clone_args zero_pidfd_args;
    memset(&zero_pidfd_args, 0, sizeof(zero_pidfd_args));
    zero_pidfd_args.flags = CLONE_PIDFD;
    zero_pidfd_args.pidfd = 0;
    zero_pidfd_args.exit_signal = SIGCHLD;
    errno = 0;
    require(syscall(__NR_clone3, &zero_pidfd_args, sizeof(zero_pidfd_args)) == -1 &&
            errno == EFAULT,
            "clone3 CLONE_PIDFD with null pidfd pointer");

    /* ── SysV 消息队列：msgget / msgsnd / msgrcv / msgctl ─────────────── */
    long msg_key = 0x57575331L;
    int msgid = msgget((key_t)msg_key, IPC_CREAT | IPC_EXCL | 0600);
    if (msgid < 0)
        fail("msgget create");
    require(msgget((key_t)msg_key, 0) == msgid,
            "msgget key lookup returns same id");
    errno = 0;
    require(msgget((key_t)msg_key, IPC_CREAT | IPC_EXCL) == -1 && errno == EEXIST,
            "msgget exclusive create conflicts");
    errno = 0;
    require(msgget((key_t)0x57575332L, 0) == -1 && errno == ENOENT,
            "msgget missing key without create");

    int msg_priv_id = msgget(IPC_PRIVATE, 0600);
    if (msg_priv_id < 0)
        fail("msgget private");
    struct wos_msg_buf msg_send = {.mtype = 1};
    memcpy(msg_send.mtext, "hello", 5);
    require(msgsnd(msg_priv_id, &msg_send, 5, 0) == 0, "msgsnd basic");
    struct wos_msg_buf msg_recv;
    memset(&msg_recv, 0, sizeof(msg_recv));
    long msg_received = msgrcv(msg_priv_id, &msg_recv, sizeof(msg_recv.mtext), 0, 0);
    require(msg_received == 5 && msg_recv.mtype == 1 &&
            memcmp(msg_recv.mtext, "hello", 5) == 0,
            "msgrcv basic roundtrip");

    /* msgtyp 选择：>0 只取同类型；<0 取 <= |msgtyp| 的最小类型；0 取队首。 */
    msg_send.mtype = 2;
    memcpy(msg_send.mtext, "two", 3);
    require(msgsnd(msg_priv_id, &msg_send, 3, 0) == 0, "msgsnd type two");
    msg_send.mtype = 1;
    memcpy(msg_send.mtext, "one", 3);
    require(msgsnd(msg_priv_id, &msg_send, 3, 0) == 0, "msgsnd type one");
    memset(&msg_recv, 0, sizeof(msg_recv));
    msg_received = msgrcv(msg_priv_id, &msg_recv, sizeof(msg_recv.mtext), 1, 0);
    require(msg_received == 3 && msg_recv.mtype == 1 &&
            memcmp(msg_recv.mtext, "one", 3) == 0,
            "msgrcv positive type selects only matching");
    msg_send.mtype = 3;
    memcpy(msg_send.mtext, "thr", 3);
    require(msgsnd(msg_priv_id, &msg_send, 3, 0) == 0, "msgsnd type three");
    memset(&msg_recv, 0, sizeof(msg_recv));
    msg_received = msgrcv(msg_priv_id, &msg_recv, sizeof(msg_recv.mtext), -2, 0);
    require(msg_received == 3 && msg_recv.mtype == 2 &&
            memcmp(msg_recv.mtext, "two", 3) == 0,
            "msgrcv negative type picks smallest at or below bound");
    memset(&msg_recv, 0, sizeof(msg_recv));
    msg_received = msgrcv(msg_priv_id, &msg_recv, sizeof(msg_recv.mtext), 0, 0);
    require(msg_received == 3 && msg_recv.mtype == 3 &&
            memcmp(msg_recv.mtext, "thr", 3) == 0,
            "msgrcv zero type consumes fifo head");

    /* MSG_NOERROR 截断；无该标志且超长返回 E2BIG 且消息保留。 */
    msg_send.mtype = 1;
    memcpy(msg_send.mtext, "abcdefghijkl", 12);
    require(msgsnd(msg_priv_id, &msg_send, 12, 0) == 0, "msgsnd long payload");
    memset(&msg_recv, 0, sizeof(msg_recv));
    msg_received = msgrcv(msg_priv_id, &msg_recv, 4, 0, MSG_NOERROR);
    require(msg_received == 4 && memcmp(msg_recv.mtext, "abcd", 4) == 0,
            "msgrcv MSG_NOERROR truncates");
    require(msgsnd(msg_priv_id, &msg_send, 12, 0) == 0, "msgsnd long again");
    errno = 0;
    require(msgrcv(msg_priv_id, &msg_recv, 4, 0, 0) == -1 && errno == E2BIG,
            "msgrcv oversized without MSG_NOERROR");
    memset(&msg_recv, 0, sizeof(msg_recv));
    msg_received = msgrcv(msg_priv_id, &msg_recv, sizeof(msg_recv.mtext), 0, 0);
    require(msg_received == 12 && memcmp(msg_recv.mtext, "abcdefghijkl", 12) == 0,
            "msgrcv oversized message retained for full read");

    /* 空队列 IPC_NOWAIT 返回 ENOMSG；非法参数返回 EINVAL。 */
    errno = 0;
    require(msgrcv(msg_priv_id, &msg_recv, sizeof(msg_recv.mtext), 0, IPC_NOWAIT) == -1 &&
            errno == ENOMSG,
            "msgrcv nonblocking empty queue");
    msg_send.mtype = 0;
    errno = 0;
    require(msgsnd(msg_priv_id, &msg_send, 1, 0) == -1 && errno == EINVAL,
            "msgsnd rejects nonpositive type");
    errno = 0;
    require(msgrcv(msg_priv_id, &msg_recv, sizeof(msg_recv.mtext), 0,
                   MSG_EXCEPT) == -1 && errno == EINVAL,
            "msgrcv rejects MSG_EXCEPT with nonpositive type");

    /* msgctl IPC_STAT 反映队列当前消息数与字节数。 */
    int msg_stat_id = msgget(IPC_PRIVATE, 0600);
    if (msg_stat_id < 0)
        fail("msgget stat queue");
    msg_send.mtype = 1;
    memcpy(msg_send.mtext, "ab", 2);
    require(msgsnd(msg_stat_id, &msg_send, 2, 0) == 0, "msgsnd stat one");
    msg_send.mtype = 2;
    memcpy(msg_send.mtext, "cd", 2);
    require(msgsnd(msg_stat_id, &msg_send, 2, 0) == 0, "msgsnd stat two");
    struct msqid_ds msg_stats;
    require(msgctl(msg_stat_id, IPC_STAT, &msg_stats) == 0 &&
            msg_stats.msg_qnum == 2 && msg_stats.msg_cbytes == 4,
            "msgctl stat qnum and cbytes");
    memset(&msg_recv, 0, sizeof(msg_recv));
    require(msgrcv(msg_stat_id, &msg_recv, sizeof(msg_recv.mtext), 0, 0) == 2,
            "msgrcv stat drain one");
    require(msgctl(msg_stat_id, IPC_STAT, &msg_stats) == 0 &&
            msg_stats.msg_qnum == 1 && msg_stats.msg_cbytes == 2,
            "msgctl stat tracks after receive");

    /* IPC_RMID 唤醒阻塞接收者并返回 EIDRM。 */
    int msg_eidrm_id = msgget(IPC_PRIVATE, 0600);
    if (msg_eidrm_id < 0)
        fail("msgget eidrm queue");
    pid_t msg_waiter = fork();
    if (msg_waiter < 0)
        fail("fork msg waiter");
    if (msg_waiter == 0) {
        struct wos_msg_buf child_buf;
        memset(&child_buf, 0, sizeof(child_buf));
        ssize_t child_received = msgrcv(msg_eidrm_id, &child_buf,
                                        sizeof(child_buf.mtext), 0, 0);
        _exit(child_received == -1 && errno == EIDRM ? 0 : 1);
    }
    struct timespec msg_grace = {.tv_sec = 0, .tv_nsec = 50000000L};
    nanosleep(&msg_grace, NULL);
    require(msgctl(msg_eidrm_id, IPC_RMID, NULL) == 0,
            "msgctl remove while receiver waits");
    require(waitpid(msg_waiter, &status, 0) == msg_waiter &&
            WIFEXITED(status) && WEXITSTATUS(status) == 0,
            "blocked msgrcv interrupted by IPC_RMID returns EIDRM");

    require(msgctl(msgid, IPC_RMID, NULL) == 0, "msgctl remove key queue");
    require(msgctl(msg_priv_id, IPC_RMID, NULL) == 0, "msgctl remove private queue");
    require(msgctl(msg_stat_id, IPC_RMID, NULL) == 0, "msgctl remove stat queue");

    /* ── SysV 信号量：原子操作、阻塞、超时、UNDO 与删除唤醒 ───────── */
    int semid = semget(IPC_PRIVATE, 2, 0600);
    if (semid < 0)
        fail("semget private");
    unsigned short sem_values[2] = {2, 0};
    union wos_semun sem_arg = {.array = sem_values};
    require(semctl(semid, 0, SETALL, sem_arg) == 0, "semctl SETALL");
    sem_values[0] = sem_values[1] = 0;
    require(semctl(semid, 0, GETALL, sem_arg) == 0 &&
            sem_values[0] == 2 && sem_values[1] == 0,
            "semctl GETALL");

    struct sembuf sem_atomic[2] = {
        {.sem_num = 0, .sem_op = -1, .sem_flg = 0},
        {.sem_num = 1, .sem_op = 3, .sem_flg = 0},
    };
    require(semop(semid, sem_atomic, 2) == 0 &&
            semctl(semid, 0, GETVAL) == 1 && semctl(semid, 1, GETVAL) == 3,
            "semop atomic multi-operation commit");

    struct sembuf sem_nowait[2] = {
        {.sem_num = 0, .sem_op = -2, .sem_flg = IPC_NOWAIT},
        {.sem_num = 1, .sem_op = 1, .sem_flg = 0},
    };
    errno = 0;
    require(semop(semid, sem_nowait, 2) == -1 && errno == EAGAIN &&
            semctl(semid, 0, GETVAL) == 1 && semctl(semid, 1, GETVAL) == 3,
            "semop failed transaction leaves every semval unchanged");

    sem_arg.val = 0;
    require(semctl(semid, 0, SETVAL, sem_arg) == 0, "semctl SETVAL zero");
    pid_t sem_waiter = fork();
    if (sem_waiter < 0)
        fail("fork sem waiter");
    if (sem_waiter == 0) {
        struct sembuf operation = {.sem_num = 0, .sem_op = -1, .sem_flg = 0};
        _exit(semop(semid, &operation, 1) == 0 ? 0 : 1);
    }
    nanosleep(&msg_grace, NULL);
    require(semctl(semid, 0, GETNCNT) >= 1, "semctl GETNCNT sees blocked waiter");
    sem_arg.val = 1;
    require(semctl(semid, 0, SETVAL, sem_arg) == 0, "SETVAL releases waiter");
    require(waitpid(sem_waiter, &status, 0) == sem_waiter &&
            WIFEXITED(status) && WEXITSTATUS(status) == 0,
            "blocked semop resumes after value becomes available");

    struct sembuf sem_timeout_op = {.sem_num = 0, .sem_op = -1, .sem_flg = 0};
    struct timespec sem_timeout = {.tv_sec = 0, .tv_nsec = 20000000L};
    errno = 0;
    require(semtimedop(semid, &sem_timeout_op, 1, &sem_timeout) == -1 &&
            errno == EAGAIN,
            "semtimedop relative timeout");

    sem_arg.val = 1;
    require(semctl(semid, 0, SETVAL, sem_arg) == 0, "SETVAL before SEM_UNDO");
    pid_t sem_undo_child = fork();
    if (sem_undo_child < 0)
        fail("fork sem undo child");
    if (sem_undo_child == 0) {
        struct sembuf operation = {.sem_num = 0, .sem_op = -1, .sem_flg = SEM_UNDO};
        _exit(semop(semid, &operation, 1) == 0 ? 0 : 1);
    }
    require(waitpid(sem_undo_child, &status, 0) == sem_undo_child &&
            WIFEXITED(status) && WEXITSTATUS(status) == 0 &&
            semctl(semid, 0, GETVAL) == 1,
            "SEM_UNDO restores adjustment when task exits");

    int sem_eidrm = semget(IPC_PRIVATE, 1, 0600);
    if (sem_eidrm < 0)
        fail("semget eidrm set");
    pid_t sem_removed_waiter = fork();
    if (sem_removed_waiter < 0)
        fail("fork removed sem waiter");
    if (sem_removed_waiter == 0) {
        struct sembuf operation = {.sem_num = 0, .sem_op = -1, .sem_flg = 0};
        int result = semop(sem_eidrm, &operation, 1);
        _exit(result == -1 && errno == EIDRM ? 0 : 1);
    }
    nanosleep(&msg_grace, NULL);
    require(semctl(sem_eidrm, 0, IPC_RMID) == 0, "semctl IPC_RMID");
    require(waitpid(sem_removed_waiter, &status, 0) == sem_removed_waiter &&
            WIFEXITED(status) && WEXITSTATUS(status) == 0,
            "IPC_RMID wakes blocked semop with EIDRM");
    require(semctl(semid, 0, IPC_RMID) == 0, "remove primary semaphore set");

    int pidfd_pipe[2];
    require(pipe(pidfd_pipe) == 0, "pidfd pipe");
    pid_t pidfd_child = fork();
    if (pidfd_child < 0)
        fail("fork pidfd child");
    if (pidfd_child == 0) {
        close(pidfd_pipe[0]);
        sleep(30);
        _exit(0);
    }
    close(pidfd_pipe[1]);
    int pidfd = syscall(__NR_pidfd_open, pidfd_child, 0);
    if (pidfd < 0)
        fail("pidfd_open");
    require((fcntl(pidfd, F_GETFD) & FD_CLOEXEC) != 0,
            "pidfd close-on-exec");
    int copied_child_fd = syscall(__NR_pidfd_getfd, pidfd, pidfd_pipe[1], 0);
    if (copied_child_fd < 0)
        fail("pidfd_getfd");
    require(write(copied_child_fd, "P", 1) == 1,
            "pidfd_getfd duplicated write endpoint");
    char pidfd_byte = 0;
    require(read(pidfd_pipe[0], &pidfd_byte, 1) == 1 && pidfd_byte == 'P',
            "pidfd_getfd shared open file description");
    require(syscall(__NR_pidfd_send_signal, pidfd, SIGTERM, NULL, 0) == 0,
            "pidfd_send_signal");
    struct pollfd pidfd_poll = {.fd = pidfd, .events = POLLIN};
    require(poll(&pidfd_poll, 1, 5000) == 1 &&
            (pidfd_poll.revents & (POLLIN | POLLHUP)) != 0,
            "pidfd poll exit readiness");
    siginfo_t pidfd_info;
    memset(&pidfd_info, 0, sizeof(pidfd_info));
    require(syscall(__NR_waitid, 3, pidfd, &pidfd_info, WEXITED, NULL) == 0 &&
            pidfd_info.si_pid == pidfd_child,
            "waitid P_PIDFD");

    unsigned char *lock_area = mmap(NULL, 8192, PROT_READ | PROT_WRITE,
                                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    require(lock_area != MAP_FAILED, "mlock anonymous mapping");
    unsigned char residency[2] = {0, 0};
    require(mincore(lock_area, 8192, residency) == 0,
            "mincore lazy mapping");
    require(madvise(lock_area, 8192, MADV_POPULATE_WRITE) == 0,
            "madvise populate write");
    residency[0] = residency[1] = 0;
    require(mincore(lock_area, 8192, residency) == 0 &&
            (residency[0] & 1) != 0 && (residency[1] & 1) != 0,
            "mincore populated residency");
    require(mlock(lock_area + 1, 4096) == 0,
            "mlock accepts unaligned address and prefaults");
    require(munlock(lock_area + 1, 4096) == 0,
            "munlock mapped range");
    require(mlockall(MCL_CURRENT | MCL_FUTURE) == 0 && munlockall() == 0,
            "mlockall current/future in non-swapping kernel");

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
    close(memfd);
    close(fixed_seal_memfd);
    close(notify_file);
    close(notify_fd);
    close(openat2_fd);
    close(openat2_dirfd);
    munmap(futex_words, 4096);
    close(futex_memfd);
    close(clone_pidfd);
    close(copied_child_fd);
    close(pidfd_pipe[0]);
    close(pidfd);
    munmap(lock_area, 8192);
    unlink(notify_old);
    rmdir(notify_dir);
    unlink(openat2_link);
    unlink(openat2_file);
    rmdir(openat2_dir);
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
    puts("[PASS] memfd: shared mmap/writeback, CLOEXEC, size/write seals and seal locking");
    puts("[PASS] inotify: create/modify/move/delete, cookies, poll and EFAULT rollback");
    puts("[PASS] openat2: versioned open_how, CLOEXEC, symlink/beneath/cache constraints");
    puts("[PASS] futex bitset/requeue/wake-op: selective, migrated and atomic conditional wake");
    puts("[PASS] futex wake-op: conditional miss and unsupported opcode");
    puts("[PASS] pidfd: clone3, open, getfd, signal, poll and waitid(P_PIDFD)");
    puts("[PASS] SysV message queues: get/snd/rcv/ctl selection, truncation and EIDRM");
    puts("[PASS] SysV semaphores: atomic ops, wait, timeout, SEM_UNDO and EIDRM");
    puts("[PASS] memory residency: mincore, MADV_POPULATE, mlock and MCL_CURRENT/FUTURE");
    return 0;
}
