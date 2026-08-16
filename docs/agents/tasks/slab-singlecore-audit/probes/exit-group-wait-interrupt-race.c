#define _GNU_SOURCE

#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdint.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

enum { WORKERS = 8 };

static atomic_int go;
static int pipe_fds[2];

static void *worker(void *opaque)
{
    uintptr_t mode = (uintptr_t)opaque;
    while (atomic_load_explicit(&go, memory_order_acquire) == 0)
        sched_yield();

    if (mode % 3 == 0) {
        char byte;
        (void)read(pipe_fds[0], &byte, 1);
    } else if (mode % 3 == 1) {
        struct timespec delay = { .tv_sec = 30, .tv_nsec = 0 };
        while (nanosleep(&delay, &delay) < 0 && errno == EINTR) {}
    } else {
        for (;;)
            sched_yield();
    }
    return 0;
}

int main(void)
{
    pthread_t workers[WORKERS];
    if (pipe(pipe_fds) != 0)
        return 2;
    atomic_init(&go, 0);
    for (uintptr_t i = 0; i < WORKERS; ++i) {
        if (pthread_create(&workers[i], 0, worker, (void *)i) != 0)
            return 3;
    }
    atomic_store_explicit(&go, 1, memory_order_release);
    syscall(SYS_exit_group, 0);
    return 4;
}
