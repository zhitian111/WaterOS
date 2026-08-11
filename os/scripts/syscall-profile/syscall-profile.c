/*
 * syscall-profile: low-noise QEMU plugin for Linux-ABI syscall workloads.
 *
 * backend=qemu uses QEMU's native linux-user syscall callbacks.  backend=ecall
 * observes user-space RISC-V ecall instructions in full-system emulation and
 * reads a7/a0..a5.  backend=auto selects between the two modes.
 */
#include <qemu-plugin.h>
#include <glib.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_VCPU 16
#define MAX_SYSCALL 512
#define MAX_ARGS 6
#define ARG_BUCKETS 66
#define MAX_ERRNO 256
#define DEFAULT_MAX_PATH 256
#define MAX_CAPTURE_PATH 4096

enum backend_kind {
    BACKEND_AUTO,
    BACKEND_QEMU,
    BACKEND_ECALL,
};

struct register_set {
    struct qemu_plugin_register *args[8];
    GByteArray *buffer;
    bool ready;
};

struct exact_key {
    uint16_t syscall_nr;
    uint8_t arg_index;
    uint64_t value;
};

struct path_key {
    uint16_t syscall_nr;
    uint8_t arg_index;
    char *path;
};

struct path_stat {
    uint64_t count;
    uint64_t last_sequence;
    uint64_t reuse[ARG_BUCKETS];
};

struct path_row {
    struct path_key *key;
    struct path_stat *stat;
};

struct exact_row {
    struct exact_key *key;
    uint64_t count;
};

static enum backend_kind requested_backend = BACKEND_AUTO;
static enum backend_kind active_backend = BACKEND_AUTO;
static const char *out_path;
static const char *target_name;
static bool system_emulation;
static bool capture_paths = true;
static uint64_t user_max = UINT64_C(0x80000000);
static unsigned int max_path = DEFAULT_MAX_PATH;
static unsigned int top_paths = 200;
static int nvcpu = 1;

static struct register_set registers[MAX_VCPU];
static GByteArray *memory_buffers[MAX_VCPU];
static GHashTable *exact_values[MAX_VCPU];
static GHashTable *path_values[MAX_VCPU];
static uint64_t call_sequence[MAX_VCPU];

static uint64_t syscall_counts[MAX_VCPU][MAX_SYSCALL];
static uint64_t arg_buckets[MAX_VCPU][MAX_SYSCALL][MAX_ARGS][ARG_BUCKETS];
static uint64_t return_success[MAX_VCPU][MAX_SYSCALL];
static uint64_t return_error[MAX_VCPU][MAX_SYSCALL];
static uint64_t errno_counts[MAX_VCPU][MAX_SYSCALL][MAX_ERRNO];
static uint64_t path_reads[MAX_VCPU][MAX_SYSCALL][MAX_ARGS];
static uint64_t path_read_failures[MAX_VCPU][MAX_SYSCALL][MAX_ARGS];
static uint64_t path_truncations[MAX_VCPU][MAX_SYSCALL][MAX_ARGS];
static uint64_t register_failures[MAX_VCPU];
static uint64_t ignored_kernel_ecalls[MAX_VCPU];

static const char *backend_name(enum backend_kind backend)
{
    switch (backend) {
    case BACKEND_QEMU:
        return "qemu";
    case BACKEND_ECALL:
        return "ecall";
    default:
        return "auto";
    }
}

static const char *syscall_name(unsigned int nr)
{
    switch (nr) {
    case 17: return "getcwd";
    case 23: return "dup";
    case 24: return "dup3";
    case 25: return "fcntl";
    case 29: return "ioctl";
    case 34: return "mkdirat";
    case 35: return "unlinkat";
    case 36: return "symlinkat";
    case 37: return "linkat";
    case 38: return "renameat";
    case 48: return "faccessat";
    case 49: return "chdir";
    case 56: return "openat";
    case 57: return "close";
    case 59: return "pipe2";
    case 61: return "getdents64";
    case 62: return "lseek";
    case 63: return "read";
    case 64: return "write";
    case 65: return "readv";
    case 66: return "writev";
    case 67: return "pread64";
    case 68: return "pwrite64";
    case 72: return "pselect6";
    case 73: return "ppoll";
    case 78: return "readlinkat";
    case 79: return "newfstatat";
    case 80: return "fstat";
    case 93: return "exit";
    case 94: return "exit_group";
    case 98: return "futex";
    case 99: return "set_robust_list";
    case 101: return "nanosleep";
    case 113: return "clock_gettime";
    case 124: return "sched_yield";
    case 129: return "kill";
    case 132: return "sigaltstack";
    case 134: return "rt_sigaction";
    case 135: return "rt_sigprocmask";
    case 160: return "uname";
    case 172: return "getpid";
    case 178: return "gettid";
    case 198: return "socket";
    case 200: return "bind";
    case 201: return "listen";
    case 202: return "accept";
    case 203: return "connect";
    case 206: return "sendto";
    case 207: return "recvfrom";
    case 214: return "brk";
    case 215: return "munmap";
    case 216: return "mremap";
    case 220: return "clone";
    case 221: return "execve";
    case 222: return "mmap";
    case 223: return "fadvise64";
    case 226: return "mprotect";
    case 233: return "madvise";
    case 260: return "wait4";
    case 261: return "prlimit64";
    case 258: return "riscv_hwprobe";
    case 259: return "riscv_flush_icache";
    case 276: return "renameat2";
    case 278: return "getrandom";
    case 281: return "execveat";
    case 291: return "statx";
    case 435: return "clone3";
    case 436: return "close_range";
    case 439: return "faccessat2";
    default: return "unknown";
    }
}

static unsigned int magnitude_bucket(uint64_t value)
{
    if (value == 0) {
        return 0;
    }
    if (value == 1) {
        return 1;
    }
    unsigned int bit = 63u - (unsigned int)__builtin_clzll(value);
    unsigned int bucket = bit + 2;
    return bucket < ARG_BUCKETS ? bucket : ARG_BUCKETS - 1;
}

static const char *bucket_label(unsigned int bucket, char *buffer, size_t len)
{
    if (bucket == 0) {
        return "zero";
    }
    if (bucket == 1) {
        return "one";
    }
    snprintf(buffer, len, "2^%u", bucket - 2);
    return buffer;
}

static guint exact_key_hash(gconstpointer pointer)
{
    const struct exact_key *key = pointer;
    uint64_t value = key->value ^ (key->value >> 33);
    value ^= ((uint64_t)key->syscall_nr << 17) | key->arg_index;
    return (guint)(value ^ (value >> 32));
}

static gboolean exact_key_equal(gconstpointer left, gconstpointer right)
{
    const struct exact_key *a = left;
    const struct exact_key *b = right;
    return a->syscall_nr == b->syscall_nr &&
           a->arg_index == b->arg_index && a->value == b->value;
}

static guint path_key_hash(gconstpointer pointer)
{
    const struct path_key *key = pointer;
    return g_str_hash(key->path) ^ ((guint)key->syscall_nr << 8) ^ key->arg_index;
}

static gboolean path_key_equal(gconstpointer left, gconstpointer right)
{
    const struct path_key *a = left;
    const struct path_key *b = right;
    return a->syscall_nr == b->syscall_nr &&
           a->arg_index == b->arg_index && strcmp(a->path, b->path) == 0;
}

static void free_path_key(gpointer pointer)
{
    struct path_key *key = pointer;
    g_free(key->path);
    g_free(key);
}

static bool track_exact_value(unsigned int nr, unsigned int arg)
{
    return (nr == 25 && arg == 1) ||
           (nr == 29 && arg == 1) ||
           (nr == 48 && arg == 2) ||
           (nr == 56 && arg == 2) ||
           (nr == 62 && arg == 2) ||
           (nr == 98 && arg == 1) ||
           (nr == 220 && arg == 0) ||
           (nr == 222 && (arg == 2 || arg == 3)) ||
           (nr == 223 && arg == 3) ||
           (nr == 226 && arg == 2) ||
           (nr == 233 && arg == 2) ||
           (nr == 260 && arg == 2) ||
           (nr == 276 && arg == 4) ||
           (nr == 291 && (arg == 2 || arg == 3)) ||
           (nr == 435 && arg == 1) ||
           (nr == 436 && arg == 2) ||
           (nr == 439 && (arg == 2 || arg == 3));
}

static unsigned int path_argument_mask(unsigned int nr)
{
    switch (nr) {
    case 34: case 35: case 48: case 56: case 78: case 79:
    case 281: case 291: case 439:
        return 1u << 1;
    case 36:
        return (1u << 0) | (1u << 2);
    case 37: case 38: case 276:
        return (1u << 1) | (1u << 3);
    case 49: case 221:
        return 1u << 0;
    default:
        return 0;
    }
}

static void record_exact(unsigned int vcpu, unsigned int nr,
                         unsigned int arg, uint64_t value)
{
    struct exact_key probe = {
        .syscall_nr = (uint16_t)nr,
        .arg_index = (uint8_t)arg,
        .value = value,
    };
    uint64_t *count = g_hash_table_lookup(exact_values[vcpu], &probe);
    if (count == NULL) {
        struct exact_key *key = g_new(struct exact_key, 1);
        *key = probe;
        count = g_new0(uint64_t, 1);
        g_hash_table_insert(exact_values[vcpu], key, count);
    }
    (*count)++;
}

static char *read_guest_path(unsigned int vcpu, uint64_t address,
                             bool *truncated)
{
    if (address == 0 || vcpu >= MAX_VCPU || memory_buffers[vcpu] == NULL) {
        return NULL;
    }
    char local[MAX_CAPTURE_PATH + 1];
    size_t copied = 0;
    *truncated = false;
    while (copied < max_path) {
        size_t page_left = 4096 - (size_t)((address + copied) & 4095);
        size_t chunk = max_path - copied;
        if (chunk > page_left) {
            chunk = page_left;
        }
        GByteArray *buffer = memory_buffers[vcpu];
        g_byte_array_set_size(buffer, 0);
        if (!qemu_plugin_read_memory_vaddr(address + copied, buffer, chunk) ||
            buffer->len < chunk) {
            return NULL;
        }
        for (size_t i = 0; i < chunk; i++) {
            local[copied++] = (char)buffer->data[i];
            if (local[copied - 1] == '\0') {
                return g_strndup(local, copied - 1);
            }
        }
    }
    local[copied] = '\0';
    *truncated = true;
    return g_strndup(local, copied);
}

static void record_path(unsigned int vcpu, unsigned int nr,
                        unsigned int arg, uint64_t address)
{
    bool truncated = false;
    char *path = read_guest_path(vcpu, address, &truncated);
    if (path == NULL) {
        path_read_failures[vcpu][nr][arg]++;
        return;
    }
    path_reads[vcpu][nr][arg]++;
    if (truncated) {
        path_truncations[vcpu][nr][arg]++;
    }

    struct path_key probe = {
        .syscall_nr = (uint16_t)nr,
        .arg_index = (uint8_t)arg,
        .path = path,
    };
    struct path_stat *stat = g_hash_table_lookup(path_values[vcpu], &probe);
    uint64_t sequence = call_sequence[vcpu];
    if (stat == NULL) {
        struct path_key *key = g_new(struct path_key, 1);
        *key = probe;
        stat = g_new0(struct path_stat, 1);
        stat->last_sequence = sequence;
        g_hash_table_insert(path_values[vcpu], key, stat);
    } else {
        uint64_t distance = sequence - stat->last_sequence;
        stat->reuse[magnitude_bucket(distance)]++;
        stat->last_sequence = sequence;
        g_free(path);
    }
    stat->count++;
}

static void record_call(unsigned int vcpu, uint64_t nr64, const uint64_t args[8])
{
    if (vcpu >= MAX_VCPU || nr64 >= MAX_SYSCALL) {
        return;
    }
    unsigned int nr = (unsigned int)nr64;
    syscall_counts[vcpu][nr]++;
    call_sequence[vcpu]++;
    for (unsigned int arg = 0; arg < MAX_ARGS; arg++) {
        arg_buckets[vcpu][nr][arg][magnitude_bucket(args[arg])]++;
        if (track_exact_value(nr, arg)) {
            record_exact(vcpu, nr, arg, args[arg]);
        }
    }
    if (capture_paths) {
        unsigned int mask = path_argument_mask(nr);
        for (unsigned int arg = 0; arg < MAX_ARGS; arg++) {
            if ((mask & (1u << arg)) != 0) {
                record_path(vcpu, nr, arg, args[arg]);
            }
        }
    }
}

static bool register_name_matches(const char *name, unsigned int index)
{
    static const char *abi_names[8] = {
        "a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7",
    };
    static const char *x_names[8] = {
        "x10", "x11", "x12", "x13", "x14", "x15", "x16", "x17",
    };
    if (strcmp(name, abi_names[index]) == 0 || strcmp(name, x_names[index]) == 0) {
        return true;
    }
    const char *slash = strrchr(name, '/');
    return slash != NULL && strcmp(slash + 1, abi_names[index]) == 0;
}

static void vcpu_init(qemu_plugin_id_t id, unsigned int vcpu)
{
    (void)id;
    if (vcpu >= MAX_VCPU) {
        return;
    }
    if ((int)vcpu + 1 > nvcpu) {
        nvcpu = (int)vcpu + 1;
    }
    memory_buffers[vcpu] = g_byte_array_sized_new(max_path);
    exact_values[vcpu] = g_hash_table_new_full(exact_key_hash, exact_key_equal,
                                                g_free, g_free);
    path_values[vcpu] = g_hash_table_new_full(path_key_hash, path_key_equal,
                                               free_path_key, g_free);
    if (active_backend != BACKEND_ECALL) {
        return;
    }

    GArray *available = qemu_plugin_get_registers();
    for (guint i = 0; i < available->len; i++) {
        qemu_plugin_reg_descriptor descriptor =
            g_array_index(available, qemu_plugin_reg_descriptor, i);
        for (unsigned int reg = 0; reg < 8; reg++) {
            if (register_name_matches(descriptor.name, reg)) {
                registers[vcpu].args[reg] = descriptor.handle;
            }
        }
    }
    registers[vcpu].ready = true;
    for (unsigned int reg = 0; reg < 8; reg++) {
        if (registers[vcpu].args[reg] == NULL) {
            registers[vcpu].ready = false;
        }
    }
    registers[vcpu].buffer = g_byte_array_sized_new(8);
    g_array_free(available, TRUE);
}

static bool read_register_u64(unsigned int vcpu, unsigned int reg, uint64_t *value)
{
    struct register_set *set = &registers[vcpu];
    if (!set->ready || set->args[reg] == NULL || set->buffer == NULL) {
        return false;
    }
    g_byte_array_set_size(set->buffer, 0);
    if (!qemu_plugin_read_register(set->args[reg], set->buffer) ||
        set->buffer->len == 0) {
        return false;
    }
    uint64_t result = 0;
    size_t bytes = set->buffer->len < sizeof(result) ? set->buffer->len : sizeof(result);
    memcpy(&result, set->buffer->data, bytes);
    *value = result;
    return true;
}

static void on_ecall(unsigned int vcpu, void *userdata)
{
    uint64_t pc = (uint64_t)(uintptr_t)userdata;
    if (vcpu >= MAX_VCPU) {
        return;
    }
    if (pc >= user_max) {
        ignored_kernel_ecalls[vcpu]++;
        return;
    }
    uint64_t values[8] = {0};
    for (unsigned int reg = 0; reg < 8; reg++) {
        if (!read_register_u64(vcpu, reg, &values[reg])) {
            register_failures[vcpu]++;
            return;
        }
    }
    record_call(vcpu, values[7], values);
}

static void tb_trans(qemu_plugin_id_t id, struct qemu_plugin_tb *tb)
{
    (void)id;
    size_t count = qemu_plugin_tb_n_insns(tb);
    for (size_t i = 0; i < count; i++) {
        struct qemu_plugin_insn *insn = qemu_plugin_tb_get_insn(tb, i);
        char *disassembly = qemu_plugin_insn_disas(insn);
        bool is_ecall = disassembly != NULL && g_str_has_prefix(disassembly, "ecall");
        g_free(disassembly);
        if (!is_ecall) {
            continue;
        }
        qemu_plugin_register_vcpu_insn_exec_cb(
            insn, on_ecall, QEMU_PLUGIN_CB_R_REGS,
            (void *)(uintptr_t)qemu_plugin_insn_vaddr(insn));
    }
}

static void native_syscall(qemu_plugin_id_t id, unsigned int vcpu, int64_t nr,
                           uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
                           uint64_t a4, uint64_t a5, uint64_t a6, uint64_t a7)
{
    (void)id;
    if (nr < 0) {
        return;
    }
    uint64_t args[8] = {a0, a1, a2, a3, a4, a5, a6, a7};
    record_call(vcpu, (uint64_t)nr, args);
}

static void native_syscall_return(qemu_plugin_id_t id, unsigned int vcpu,
                                  int64_t nr, int64_t result)
{
    (void)id;
    if (vcpu >= MAX_VCPU || nr < 0 || nr >= MAX_SYSCALL) {
        return;
    }
    if (result < 0 && result >= -MAX_ERRNO) {
        unsigned int error = (unsigned int)(-result);
        return_error[vcpu][nr]++;
        errno_counts[vcpu][nr][error]++;
    } else {
        return_success[vcpu][nr]++;
    }
}

static void merge_exact_table(GHashTable *destination, GHashTable *source)
{
    GHashTableIter iterator;
    gpointer key_pointer;
    gpointer value_pointer;
    g_hash_table_iter_init(&iterator, source);
    while (g_hash_table_iter_next(&iterator, &key_pointer, &value_pointer)) {
        struct exact_key *source_key = key_pointer;
        uint64_t *source_count = value_pointer;
        uint64_t *count = g_hash_table_lookup(destination, source_key);
        if (count == NULL) {
            struct exact_key *key = g_new(struct exact_key, 1);
            *key = *source_key;
            count = g_new0(uint64_t, 1);
            g_hash_table_insert(destination, key, count);
        }
        *count += *source_count;
    }
}

static void merge_path_table(GHashTable *destination, GHashTable *source)
{
    GHashTableIter iterator;
    gpointer key_pointer;
    gpointer value_pointer;
    g_hash_table_iter_init(&iterator, source);
    while (g_hash_table_iter_next(&iterator, &key_pointer, &value_pointer)) {
        struct path_key *source_key = key_pointer;
        struct path_stat *source_stat = value_pointer;
        struct path_stat *stat = g_hash_table_lookup(destination, source_key);
        if (stat == NULL) {
            struct path_key *key = g_new(struct path_key, 1);
            key->syscall_nr = source_key->syscall_nr;
            key->arg_index = source_key->arg_index;
            key->path = g_strdup(source_key->path);
            stat = g_new0(struct path_stat, 1);
            g_hash_table_insert(destination, key, stat);
        }
        stat->count += source_stat->count;
        for (unsigned int bucket = 0; bucket < ARG_BUCKETS; bucket++) {
            stat->reuse[bucket] += source_stat->reuse[bucket];
        }
    }
}

static int compare_path_rows(const void *left, const void *right)
{
    const struct path_row *a = left;
    const struct path_row *b = right;
    if (a->stat->count != b->stat->count) {
        return a->stat->count < b->stat->count ? 1 : -1;
    }
    return strcmp(a->key->path, b->key->path);
}

static int compare_exact_rows(const void *left, const void *right)
{
    const struct exact_row *a = left;
    const struct exact_row *b = right;
    if (a->count != b->count) {
        return a->count < b->count ? 1 : -1;
    }
    if (a->key->syscall_nr != b->key->syscall_nr) {
        return a->key->syscall_nr < b->key->syscall_nr ? -1 : 1;
    }
    return a->key->arg_index < b->key->arg_index ? -1 : 1;
}

static void print_escaped_path(FILE *out, const char *path)
{
    for (const unsigned char *cursor = (const unsigned char *)path; *cursor != 0; cursor++) {
        switch (*cursor) {
        case '\\': fputs("\\\\", out); break;
        case '\t': fputs("\\t", out); break;
        case '\n': fputs("\\n", out); break;
        case '\r': fputs("\\r", out); break;
        default:
            if (*cursor < 0x20 || *cursor == 0x7f) {
                fprintf(out, "\\x%02x", *cursor);
            } else {
                fputc(*cursor, out);
            }
        }
    }
}

static void plugin_exit(qemu_plugin_id_t id, void *userdata)
{
    (void)id;
    (void)userdata;
    FILE *out = stderr;
    if (out_path != NULL) {
        out = fopen(out_path, "w");
        if (out == NULL) {
            out = stderr;
        }
    }

    uint64_t total = 0;
    for (int vcpu = 0; vcpu < nvcpu; vcpu++) {
        for (unsigned int nr = 0; nr < MAX_SYSCALL; nr++) {
            total += syscall_counts[vcpu][nr];
        }
    }
    fprintf(out,
            "# syscall-profile version=1 backend=%s requested=%s system=%d "
            "target=%s total=%" PRIu64 " vcpus=%d max_path=%u\n",
            backend_name(active_backend), backend_name(requested_backend),
            system_emulation ? 1 : 0, target_name, total, nvcpu, max_path);
    fprintf(out, "# S nr name total per-vcpu...\n");
    fprintf(out, "# A nr arg bucket count\n");
    fprintf(out, "# V nr arg value count\n");
    fprintf(out, "# R nr success error\n");
    fprintf(out, "# E nr errno count\n");
    fprintf(out, "# P nr arg reads unique repeats failures truncated\n");
    fprintf(out, "# D nr arg reuse_bucket count\n");
    fprintf(out, "# PV nr arg count path\n");

    for (unsigned int nr = 0; nr < MAX_SYSCALL; nr++) {
        uint64_t count = 0;
        for (int vcpu = 0; vcpu < nvcpu; vcpu++) {
            count += syscall_counts[vcpu][nr];
        }
        if (count == 0) {
            continue;
        }
        fprintf(out, "S\t%u\t%s\t%" PRIu64, nr, syscall_name(nr), count);
        for (int vcpu = 0; vcpu < nvcpu; vcpu++) {
            fprintf(out, "\t%" PRIu64, syscall_counts[vcpu][nr]);
        }
        fputc('\n', out);
        for (unsigned int arg = 0; arg < MAX_ARGS; arg++) {
            for (unsigned int bucket = 0; bucket < ARG_BUCKETS; bucket++) {
                uint64_t bucket_count = 0;
                for (int vcpu = 0; vcpu < nvcpu; vcpu++) {
                    bucket_count += arg_buckets[vcpu][nr][arg][bucket];
                }
                if (bucket_count != 0) {
                    char label[16];
                    fprintf(out, "A\t%u\t%u\t%s\t%" PRIu64 "\n",
                            nr, arg, bucket_label(bucket, label, sizeof(label)),
                            bucket_count);
                }
            }
        }
        uint64_t successes = 0;
        uint64_t errors = 0;
        for (int vcpu = 0; vcpu < nvcpu; vcpu++) {
            successes += return_success[vcpu][nr];
            errors += return_error[vcpu][nr];
        }
        if (successes != 0 || errors != 0) {
            fprintf(out, "R\t%u\t%" PRIu64 "\t%" PRIu64 "\n",
                    nr, successes, errors);
            for (unsigned int error = 1; error < MAX_ERRNO; error++) {
                uint64_t error_count = 0;
                for (int vcpu = 0; vcpu < nvcpu; vcpu++) {
                    error_count += errno_counts[vcpu][nr][error];
                }
                if (error_count != 0) {
                    fprintf(out, "E\t%u\t%u\t%" PRIu64 "\n",
                            nr, error, error_count);
                }
            }
        }
    }

    GHashTable *merged_exact = g_hash_table_new_full(exact_key_hash, exact_key_equal,
                                                       g_free, g_free);
    GHashTable *merged_paths = g_hash_table_new_full(path_key_hash, path_key_equal,
                                                      free_path_key, g_free);
    for (int vcpu = 0; vcpu < nvcpu; vcpu++) {
        if (exact_values[vcpu] != NULL) {
            merge_exact_table(merged_exact, exact_values[vcpu]);
        }
        if (path_values[vcpu] != NULL) {
            merge_path_table(merged_paths, path_values[vcpu]);
        }
    }

    size_t exact_len = g_hash_table_size(merged_exact);
    struct exact_row *exact_rows = g_new0(struct exact_row, exact_len);
    GHashTableIter iterator;
    gpointer key_pointer;
    gpointer value_pointer;
    size_t index = 0;
    g_hash_table_iter_init(&iterator, merged_exact);
    while (g_hash_table_iter_next(&iterator, &key_pointer, &value_pointer)) {
        exact_rows[index].key = key_pointer;
        exact_rows[index].count = *(uint64_t *)value_pointer;
        index++;
    }
    qsort(exact_rows, exact_len, sizeof(*exact_rows), compare_exact_rows);
    for (index = 0; index < exact_len; index++) {
        fprintf(out, "V\t%u\t%u\t0x%016" PRIx64 "\t%" PRIu64 "\n",
                exact_rows[index].key->syscall_nr,
                exact_rows[index].key->arg_index,
                exact_rows[index].key->value,
                exact_rows[index].count);
    }

    uint64_t path_total[MAX_SYSCALL][MAX_ARGS] = {{0}};
    uint64_t path_unique[MAX_SYSCALL][MAX_ARGS] = {{0}};
    uint64_t path_reuse[MAX_SYSCALL][MAX_ARGS][ARG_BUCKETS] = {{{0}}};
    size_t path_len = g_hash_table_size(merged_paths);
    struct path_row *path_rows = g_new0(struct path_row, path_len);
    index = 0;
    g_hash_table_iter_init(&iterator, merged_paths);
    while (g_hash_table_iter_next(&iterator, &key_pointer, &value_pointer)) {
        struct path_key *key = key_pointer;
        struct path_stat *stat = value_pointer;
        path_rows[index].key = key;
        path_rows[index].stat = stat;
        index++;
        path_total[key->syscall_nr][key->arg_index] += stat->count;
        path_unique[key->syscall_nr][key->arg_index]++;
        for (unsigned int bucket = 0; bucket < ARG_BUCKETS; bucket++) {
            path_reuse[key->syscall_nr][key->arg_index][bucket] += stat->reuse[bucket];
        }
    }
    qsort(path_rows, path_len, sizeof(*path_rows), compare_path_rows);

    for (unsigned int nr = 0; nr < MAX_SYSCALL; nr++) {
        for (unsigned int arg = 0; arg < MAX_ARGS; arg++) {
            uint64_t reads = 0;
            uint64_t failures = 0;
            uint64_t truncations = 0;
            for (int vcpu = 0; vcpu < nvcpu; vcpu++) {
                reads += path_reads[vcpu][nr][arg];
                failures += path_read_failures[vcpu][nr][arg];
                truncations += path_truncations[vcpu][nr][arg];
            }
            if (reads == 0 && failures == 0) {
                continue;
            }
            uint64_t repeats = path_total[nr][arg] - path_unique[nr][arg];
            fprintf(out,
                    "P\t%u\t%u\t%" PRIu64 "\t%" PRIu64 "\t%" PRIu64
                    "\t%" PRIu64 "\t%" PRIu64 "\n",
                    nr, arg, reads, path_unique[nr][arg], repeats,
                    failures, truncations);
            for (unsigned int bucket = 0; bucket < ARG_BUCKETS; bucket++) {
                if (path_reuse[nr][arg][bucket] != 0) {
                    char label[16];
                    fprintf(out, "D\t%u\t%u\t%s\t%" PRIu64 "\n",
                            nr, arg, bucket_label(bucket, label, sizeof(label)),
                            path_reuse[nr][arg][bucket]);
                }
            }
        }
    }
    size_t paths_to_print = path_len < top_paths ? path_len : top_paths;
    for (index = 0; index < paths_to_print; index++) {
        fprintf(out, "PV\t%u\t%u\t%" PRIu64 "\t",
                path_rows[index].key->syscall_nr,
                path_rows[index].key->arg_index,
                path_rows[index].stat->count);
        print_escaped_path(out, path_rows[index].key->path);
        fputc('\n', out);
    }

    for (int vcpu = 0; vcpu < nvcpu; vcpu++) {
        if (register_failures[vcpu] != 0 || ignored_kernel_ecalls[vcpu] != 0) {
            fprintf(out, "X\t%d\tregister_failures\t%" PRIu64
                         "\tignored_kernel_ecalls\t%" PRIu64 "\n",
                    vcpu, register_failures[vcpu], ignored_kernel_ecalls[vcpu]);
        }
    }

    g_free(exact_rows);
    g_free(path_rows);
    g_hash_table_destroy(merged_exact);
    g_hash_table_destroy(merged_paths);
    if (out != stderr) {
        fclose(out);
        fprintf(stderr, "syscall-profile: backend=%s calls=%" PRIu64 " -> %s\n",
                backend_name(active_backend), total, out_path);
    }
}

static bool parse_bool(const char *value, bool *result)
{
    if (strcmp(value, "1") == 0 || strcmp(value, "true") == 0 ||
        strcmp(value, "on") == 0) {
        *result = true;
        return true;
    }
    if (strcmp(value, "0") == 0 || strcmp(value, "false") == 0 ||
        strcmp(value, "off") == 0) {
        *result = false;
        return true;
    }
    return false;
}

QEMU_PLUGIN_EXPORT int qemu_plugin_version = QEMU_PLUGIN_VERSION;

QEMU_PLUGIN_EXPORT int qemu_plugin_install(qemu_plugin_id_t id,
                                           const qemu_info_t *info,
                                           int argc, char **argv)
{
    for (int i = 0; i < argc; i++) {
        if (strncmp(argv[i], "out=", 4) == 0) {
            out_path = argv[i] + 4;
        } else if (strncmp(argv[i], "backend=", 8) == 0) {
            const char *value = argv[i] + 8;
            if (strcmp(value, "auto") == 0) {
                requested_backend = BACKEND_AUTO;
            } else if (strcmp(value, "qemu") == 0) {
                requested_backend = BACKEND_QEMU;
            } else if (strcmp(value, "ecall") == 0) {
                requested_backend = BACKEND_ECALL;
            } else {
                fprintf(stderr, "syscall-profile: invalid backend=%s\n", value);
                return -1;
            }
        } else if (strncmp(argv[i], "paths=", 6) == 0) {
            if (!parse_bool(argv[i] + 6, &capture_paths)) {
                fprintf(stderr, "syscall-profile: invalid paths option\n");
                return -1;
            }
        } else if (strncmp(argv[i], "user_max=", 9) == 0) {
            user_max = strtoull(argv[i] + 9, NULL, 0);
        } else if (strncmp(argv[i], "max_path=", 9) == 0) {
            unsigned long value = strtoul(argv[i] + 9, NULL, 0);
            if (value == 0 || value > MAX_CAPTURE_PATH) {
                fprintf(stderr, "syscall-profile: max_path must be 1..%d\n",
                        MAX_CAPTURE_PATH);
                return -1;
            }
            max_path = (unsigned int)value;
        } else if (strncmp(argv[i], "top_paths=", 10) == 0) {
            top_paths = (unsigned int)strtoul(argv[i] + 10, NULL, 0);
        }
    }

    system_emulation = info->system_emulation;
    target_name = g_strdup(info->target_name);
    active_backend = requested_backend;
    if (active_backend == BACKEND_AUTO) {
        active_backend = system_emulation ? BACKEND_ECALL : BACKEND_QEMU;
    }
    if (active_backend == BACKEND_ECALL &&
        strcmp(info->target_name, "riscv64") != 0 &&
        strcmp(info->target_name, "riscv") != 0) {
        fprintf(stderr,
                "syscall-profile: ecall backend currently supports RISC-V only "
                "(target=%s)\n", info->target_name);
        return -1;
    }

    qemu_plugin_register_vcpu_init_cb(id, vcpu_init);
    if (active_backend == BACKEND_QEMU) {
        qemu_plugin_register_vcpu_syscall_cb(id, native_syscall);
        qemu_plugin_register_vcpu_syscall_ret_cb(id, native_syscall_return);
    } else {
        qemu_plugin_register_vcpu_tb_trans_cb(id, tb_trans);
    }
    qemu_plugin_register_atexit_cb(id, plugin_exit, NULL);
    return 0;
}
