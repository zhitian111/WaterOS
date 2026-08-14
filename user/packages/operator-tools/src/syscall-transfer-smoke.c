#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/wait.h>
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

#define IOPRIO_WHO_PROCESS 1
#define IOPRIO_CLASS_BE 2
#define IOPRIO_PRIO_VALUE(class_, data_) (((class_) << 13) | (data_))

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
    unlink(source_path);
    unlink(output_path);
    puts("[PASS] copy_file_range: content, offsets, flags");
    puts("[PASS] sendfile: positional isolation and sequential lease progress");
    puts("[PASS] splice: file->pipe->file, positions, validation");
    puts("[PASS] tee/vmsplice: duplicate without consume and iovec input");
    puts("[PASS] ioprio: set/get and fork inheritance");
    return 0;
}
