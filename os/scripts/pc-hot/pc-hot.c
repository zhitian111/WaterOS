/*
 * pc-hot: QEMU TCG plugin that counts executed instructions per guest PC.
 *
 * Thread safety: each vCPU keeps its own hash table (TCG callbacks run
 * concurrently per vCPU), tables are merged once at exit.
 *
 * Usage:
 *   qemu-system-* -plugin file=/path/pc-hot.so,out=/tmp/pcs.txt ...
 *
 * Output (out=file, one line per distinct PC, sorted by count desc):
 *   # pc-hot: <total insns>, <distinct pcs>
 *   <count> <pc> <v0> <v1> ... <vN-1>
 *
 * Without out=, prints only the top-100 PCs to stderr.
 *
 * Compile (needs /usr/include/qemu-plugin.h and glib):
 *   gcc $(pkg-config --cflags glib-2.0) -shared -fPIC -O2 -o pc-hot.so pc-hot.c
 */
#include <qemu-plugin.h>
#include <glib.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_VCPU 16

struct pc_stat {
    uint64_t pc;
    uint64_t per_vcpu[MAX_VCPU];
    uint64_t total;
};

static GHashTable *tabs[MAX_VCPU];
static GHashTable *merged;
static const char *out_path;
static int nvcpu = 1;
static int fast_mode;
static GMutex fast_lock;

struct insn_entry {
    uint64_t vaddr;
    struct qemu_plugin_scoreboard *score;
};
static GArray *fast_entries;

static void on_insn(unsigned int vcpu_index, void *udata)
{
    uint64_t pc = (uint64_t)(uintptr_t)udata;
    GHashTable *tab = tabs[vcpu_index];
    uint64_t *c = g_hash_table_lookup(tab, (gpointer)(uintptr_t)pc);
    if (c == NULL) {
        c = g_new0(uint64_t, 1);
        g_hash_table_insert(tab, (gpointer)(uintptr_t)pc, c);
    }
    (*c)++;
}

static void vcpu_init(qemu_plugin_id_t id, unsigned int vcpu_index)
{
    if ((int)vcpu_index + 1 > nvcpu) {
        nvcpu = vcpu_index + 1;
    }
}

static void tb_trans(qemu_plugin_id_t id, struct qemu_plugin_tb *tb)
{
    size_t n = qemu_plugin_tb_n_insns(tb);
    for (size_t i = 0; i < n; i++) {
        struct qemu_plugin_insn *insn = qemu_plugin_tb_get_insn(tb, i);
        if (fast_mode) {
            g_mutex_lock(&fast_lock);
            struct insn_entry e = {
                .vaddr = qemu_plugin_insn_vaddr(insn),
                .score = qemu_plugin_scoreboard_new(sizeof(uint64_t)),
            };
            g_array_append_val(fast_entries, e);
            g_mutex_unlock(&fast_lock);
            qemu_plugin_register_vcpu_insn_exec_inline_per_vcpu(
                insn, QEMU_PLUGIN_INLINE_ADD_U64,
                qemu_plugin_scoreboard_u64(e.score), 1);
            continue;
        }
        qemu_plugin_register_vcpu_insn_exec_cb(
            insn, on_insn, QEMU_PLUGIN_CB_NO_REGS,
            (void *)(uintptr_t)qemu_plugin_insn_vaddr(insn));
    }
}

static void fast_accumulate(void)
{
    for (guint i = 0; i < fast_entries->len; i++) {
        struct insn_entry *e = &g_array_index(fast_entries, struct insn_entry, i);
        for (int v = 0; v < nvcpu; v++) {
            uint64_t c = qemu_plugin_u64_get(
                qemu_plugin_scoreboard_u64(e->score), v);
            if (c == 0) {
                continue;
            }
            GHashTable *tab = tabs[v];
            uint64_t *pc = g_hash_table_lookup(
                tab, (gpointer)(uintptr_t)e->vaddr);
            if (pc == NULL) {
                pc = g_new0(uint64_t, 1);
                g_hash_table_insert(tab, (gpointer)(uintptr_t)e->vaddr, pc);
            }
            *pc += c;
        }
    }
}

static void merge_one(gpointer key, gpointer value, gpointer userdata)
{
    int v = (int)(intptr_t)userdata;
    struct pc_stat *s = g_hash_table_lookup(merged, key);
    if (s == NULL) {
        s = g_new0(struct pc_stat, 1);
        s->pc = (uint64_t)(uintptr_t)key;
        g_hash_table_insert(merged, key, s);
    }
    s->per_vcpu[v] = *(uint64_t *)value;
    s->total += *(uint64_t *)value;
}

static int cmp(const void *a, const void *b)
{
    const struct pc_stat *x = *(const struct pc_stat * const *)a;
    const struct pc_stat *y = *(const struct pc_stat * const *)b;
    return x->total < y->total ? 1 : x->total > y->total ? -1 : 0;
}

static void plugin_exit(qemu_plugin_id_t id, void *p)
{
    if (fast_mode) {
        fast_accumulate();
    }
    merged = g_hash_table_new(g_direct_hash, g_direct_equal);
    for (int v = 0; v < MAX_VCPU; v++) {
        if (tabs[v] != NULL) {
            g_hash_table_foreach(tabs[v], merge_one, (gpointer)(intptr_t)v);
        }
    }

    GList *vals = g_hash_table_get_values(merged);
    int n = g_list_length(vals);
    struct pc_stat **arr = g_new(struct pc_stat *, n);
    GList *it;
    int i = 0;
    for (it = vals; it != NULL; it = it->next) {
        arr[i++] = it->data;
    }
    qsort(arr, n, sizeof(*arr), cmp);

    uint64_t total = 0;
    for (i = 0; i < n; i++) {
        total += arr[i]->total;
    }

    FILE *out = stderr;
    if (out_path != NULL) {
        out = fopen(out_path, "w");
        if (out == NULL) {
            out = stderr;
        }
    }
    fprintf(out, "# pc-hot: %" PRIu64 " insns, %d distinct pcs\n", total, n);
    int top = (out == stderr) ? (n < 100 ? n : 100) : n;
    for (i = 0; i < top; i++) {
        struct pc_stat *s = arr[i];
        fprintf(out, "%10" PRIu64 " 0x%016" PRIx64, s->total, s->pc);
        for (int v = 0; v < nvcpu; v++) {
            fprintf(out, " %" PRIu64, s->per_vcpu[v]);
        }
        fprintf(out, "\n");
    }
    if (out != stderr) {
        fclose(out);
        fprintf(stderr, "pc-hot: %d pcs -> %s\n", n, out_path);
    }
}

QEMU_PLUGIN_EXPORT int qemu_plugin_version = QEMU_PLUGIN_VERSION;

QEMU_PLUGIN_EXPORT int qemu_plugin_install(qemu_plugin_id_t id,
                                           const qemu_info_t *info,
                                           int argc, char **argv)
{
    for (int i = 0; i < argc; i++) {
        if (strncmp(argv[i], "out=", 4) == 0) {
            out_path = argv[i] + 4;
        }
        if (strcmp(argv[i], "fast=1") == 0) {
            fast_mode = 1;
        }
    }
    for (int v = 0; v < MAX_VCPU; v++) {
        tabs[v] = g_hash_table_new(g_direct_hash, g_direct_equal);
    }
    if (fast_mode) {
        fast_entries = g_array_new(FALSE, FALSE, sizeof(struct insn_entry));
    }
    qemu_plugin_register_vcpu_init_cb(id, vcpu_init);
    qemu_plugin_register_vcpu_tb_trans_cb(id, tb_trans);
    qemu_plugin_register_atexit_cb(id, plugin_exit, NULL);
    return 0;
}
