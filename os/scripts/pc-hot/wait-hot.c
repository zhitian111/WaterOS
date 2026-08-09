/*
 * wait-hot: QEMU TCG plugin for per-vCPU wait/idle and blocking-syscall time.
 *
 * This is a profiling helper only. It does not modify guest or kernel code.
 * It uses host monotonic time because QEMU's plugin API has no read-only
 * virtual-clock query. For reducing wall-clock final time this is the quantity
 * that matters.
 *
 * Usage:
 *   qemu-system-riscv64 ... \
 *     -plugin file=/path/wait-hot-rv.so,out=/tmp/wait-hot.txt
 */
#include <qemu-plugin.h>
#include <glib.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define MAX_VCPU 16

struct sys_stat {
    uint64_t key;
    uint64_t count;
    uint64_t ns;
};

static GHashTable *sys_stats;
static const char *out_path;
static int nvcpu = 1;
static int arch_is_loong;
static uint64_t idle_start[MAX_VCPU];
static uint64_t idle_total[MAX_VCPU];
static uint64_t idle_count[MAX_VCPU];
static uint64_t wfi_pc[MAX_VCPU];
static uint64_t sys_start[MAX_VCPU];
static int64_t sys_num[MAX_VCPU];
static int sys_active[MAX_VCPU];

static uint64_t now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

static void on_wfi(unsigned int vcpu_index, void *udata)
{
    if (vcpu_index >= MAX_VCPU) {
        return;
    }
    wfi_pc[vcpu_index] = (uint64_t)(uintptr_t)udata;
    if (idle_start[vcpu_index] == 0) {
        idle_start[vcpu_index] = now_ns();
    }
}

static void on_discon(qemu_plugin_id_t id,
                      unsigned int vcpu_index,
                      enum qemu_plugin_discon_type type,
                      uint64_t from_pc,
                      uint64_t to_pc)
{
    (void)id;
    (void)to_pc;
    if (vcpu_index >= MAX_VCPU ||
        (type & QEMU_PLUGIN_DISCON_INTERRUPT) == 0 ||
        idle_start[vcpu_index] == 0) {
        return;
    }
    wfi_pc[vcpu_index] = from_pc;
}

static void vcpu_init(qemu_plugin_id_t id, unsigned int vcpu_index)
{
    (void)id;
    if ((int)vcpu_index + 1 > nvcpu) {
        nvcpu = vcpu_index + 1;
    }
}

static void vcpu_idle(qemu_plugin_id_t id, unsigned int vcpu_index)
{
    (void)id;
    if (vcpu_index < MAX_VCPU && idle_start[vcpu_index] == 0) {
        idle_start[vcpu_index] = now_ns();
    }
}

static void vcpu_resume(qemu_plugin_id_t id, unsigned int vcpu_index)
{
    (void)id;
    if (vcpu_index >= MAX_VCPU || idle_start[vcpu_index] == 0) {
        return;
    }
    uint64_t now = now_ns();
    if (now > idle_start[vcpu_index]) {
        idle_total[vcpu_index] += now - idle_start[vcpu_index];
        idle_count[vcpu_index]++;
    }
    idle_start[vcpu_index] = 0;
}

static void on_syscall_enter(qemu_plugin_id_t id,
                             unsigned int vcpu_index,
                             int64_t num,
                             uint64_t a1,
                             uint64_t a2,
                             uint64_t a3,
                             uint64_t a4,
                             uint64_t a5,
                             uint64_t a6,
                             uint64_t a7,
                             uint64_t a8)
{
    (void)id;
    (void)a1;
    (void)a2;
    (void)a3;
    (void)a4;
    (void)a5;
    (void)a6;
    (void)a7;
    (void)a8;
    if (vcpu_index >= MAX_VCPU || sys_active[vcpu_index]) {
        return;
    }
    sys_start[vcpu_index] = now_ns();
    sys_num[vcpu_index] = num;
    sys_active[vcpu_index] = 1;
}

static void on_syscall_ret(qemu_plugin_id_t id,
                           unsigned int vcpu_index,
                           int64_t num,
                           int64_t ret)
{
    (void)id;
    (void)ret;
    if (vcpu_index >= MAX_VCPU || !sys_active[vcpu_index] ||
        sys_num[vcpu_index] != num) {
        return;
    }
    uint64_t now = now_ns();
    if (now > sys_start[vcpu_index]) {
        uint64_t key = ((uint64_t)vcpu_index << 32) |
                       ((uint64_t)(uint32_t)num);
        struct sys_stat *stat = g_hash_table_lookup(sys_stats,
                                                    (gpointer)(uintptr_t)key);
        if (stat == NULL) {
            stat = g_new0(struct sys_stat, 1);
            stat->key = key;
            g_hash_table_insert(sys_stats,
                                (gpointer)(uintptr_t)key,
                                stat);
        }
        stat->count++;
        stat->ns += now - sys_start[vcpu_index];
    }
    sys_active[vcpu_index] = 0;
}

static void tb_trans(qemu_plugin_id_t id, struct qemu_plugin_tb *tb)
{
    (void)id;
    size_t n = qemu_plugin_tb_n_insns(tb);
    for (size_t i = 0; i < n; i++) {
        struct qemu_plugin_insn *insn = qemu_plugin_tb_get_insn(tb, i);
        int is_wait = 0;
        if (arch_is_loong) {
            char *text = qemu_plugin_insn_disas(insn);
            is_wait = text != NULL &&
                      (strstr(text, "idle") != NULL ||
                       strstr(text, "wfi") != NULL);
            if (text != NULL) {
                g_free(text);
            }
        } else {
            uint8_t data[4] = {0};
            size_t len = qemu_plugin_insn_data(insn, data, sizeof(data));
            static const uint8_t riscv_wfi[4] = {0x73, 0x00, 0x50, 0x10};
            is_wait = len == sizeof(data) && memcmp(data, riscv_wfi, sizeof(data)) == 0;
        }
        if (is_wait) {
            qemu_plugin_register_vcpu_insn_exec_cb(
                insn,
                on_wfi,
                QEMU_PLUGIN_CB_NO_REGS,
                (void *)(uintptr_t)qemu_plugin_insn_vaddr(insn));
        }
    }
}

static int cmp_sys(const void *a, const void *b)
{
    const struct sys_stat *x = *(const struct sys_stat * const *)a;
    const struct sys_stat *y = *(const struct sys_stat * const *)b;
    return x->ns < y->ns ? 1 : x->ns > y->ns ? -1 : 0;
}

static void plugin_exit(qemu_plugin_id_t id, void *p)
{
    (void)id;
    (void)p;
    uint64_t now = now_ns();
    for (int v = 0; v < nvcpu; v++) {
        if (idle_start[v] != 0 && now > idle_start[v]) {
            idle_total[v] += now - idle_start[v];
            idle_count[v]++;
            idle_start[v] = 0;
        }
    }

    FILE *out = stderr;
    if (out_path != NULL) {
        out = fopen(out_path, "w");
        if (out == NULL) {
            out = stderr;
        }
    }
    fprintf(out, "# wait-hot: host_wall_ms=%.1f\n",
            (double)now / 1000000.0);
    for (int v = 0; v < nvcpu; v++) {
        fprintf(out, "cpu %d idle_ms=%.3f idle_count=%" PRIu64 " wfi_pc=0x%016" PRIx64 "\n",
                v,
                (double)idle_total[v] / 1000000.0,
                idle_count[v],
                wfi_pc[v]);
    }

    GList *vals = g_hash_table_get_values(sys_stats);
    int n = g_list_length(vals);
    struct sys_stat **arr = g_new(struct sys_stat *, n);
    GList *it;
    int i = 0;
    for (it = vals; it != NULL; it = it->next) {
        arr[i++] = it->data;
    }
    qsort(arr, n, sizeof(*arr), cmp_sys);
    for (i = 0; i < n; i++) {
        struct sys_stat *s = arr[i];
        fprintf(out, "syscall cpu=%" PRIu64 " nr=%" PRIu64 " count=%" PRIu64 " ns=%" PRIu64 " ms=%.3f\n",
                s->key >> 32,
                s->key & 0xffffffffULL,
                s->count,
                s->ns,
                (double)s->ns / 1000000.0);
    }
    if (out != stderr) {
        fclose(out);
        fprintf(stderr, "wait-hot: %d pcs -> %s\n", n, out_path);
    }
}

QEMU_PLUGIN_EXPORT int qemu_plugin_version = QEMU_PLUGIN_VERSION;

QEMU_PLUGIN_EXPORT int qemu_plugin_install(qemu_plugin_id_t id,
                                           const qemu_info_t *info,
                                           int argc, char **argv)
{
    (void)info;
    arch_is_loong = info != NULL && info->target_name != NULL &&
                    strstr(info->target_name, "loong") != NULL;
    for (int i = 0; i < argc; i++) {
        if (strncmp(argv[i], "out=", 4) == 0) {
            out_path = argv[i] + 4;
        }
    }
    sys_stats = g_hash_table_new(g_direct_hash, g_direct_equal);
    qemu_plugin_register_vcpu_init_cb(id, vcpu_init);
    qemu_plugin_register_vcpu_idle_cb(id, vcpu_idle);
    qemu_plugin_register_vcpu_resume_cb(id, vcpu_resume);
    qemu_plugin_register_vcpu_tb_trans_cb(id, tb_trans);
    qemu_plugin_register_vcpu_discon_cb(id,
                                        QEMU_PLUGIN_DISCON_INTERRUPT,
                                        on_discon);
    qemu_plugin_register_vcpu_syscall_cb(id, on_syscall_enter);
    qemu_plugin_register_vcpu_syscall_ret_cb(id, on_syscall_ret);
    qemu_plugin_register_atexit_cb(id, plugin_exit, NULL);
    return 0;
}
