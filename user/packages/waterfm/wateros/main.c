/* WaterFM: deliberately small Nano-X file manager for WaterOS. */
#include <nano-X.h>

#include <dirent.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

enum { WIDTH = 720, HEIGHT = 520, TOOLBAR = 32, ROW = 20, LIST_TOP = TOOLBAR + 50,
       LIST_BOTTOM = HEIGHT - 30, SCROLL_WIDTH = 14, MAX_ENTRIES = 256, NAME_MAX_LEN = 255 };

struct entry { char name[NAME_MAX_LEN + 1]; int is_dir; int executable; };
static struct entry entries[MAX_ENTRIES];
static size_t entry_count;
static size_t scroll_first;
static char cwd[1024] = "/";
static int selected = -1;
static char status[160] = "Double-click a directory, executable, or .gba ROM";
static int editing;
static int edit_rename;
static char edit_text[NAME_MAX_LEN + 1];
static size_t edit_len;
static GR_WINDOW_ID backbuffer;

static size_t visible_rows(void) { return (LIST_BOTTOM - LIST_TOP) / ROW; }

static size_t max_scroll(void) {
    size_t rows = visible_rows();
    return entry_count > rows ? entry_count - rows : 0;
}

static void clamp_scroll(void) {
    if (scroll_first > max_scroll()) scroll_first = max_scroll();
}

static int compare_entries(const void *a, const void *b) {
    const struct entry *left = a, *right = b;
    if (left->is_dir != right->is_dir) return right->is_dir - left->is_dir;
    return strcmp(left->name, right->name);
}

static void join_path(char *output, size_t size, const char *name) {
    if (!strcmp(cwd, "/")) snprintf(output, size, "/%s", name);
    else snprintf(output, size, "%s/%s", cwd, name);
}

static void load_directory(void) {
    DIR *dir = opendir(cwd);
    entry_count = 0;
    selected = -1;
    scroll_first = 0;
    if (!dir) { snprintf(status, sizeof(status), "Cannot open %s: %s", cwd, strerror(errno)); return; }
    struct dirent *item;
    while ((item = readdir(dir)) && entry_count < MAX_ENTRIES) {
        if (!strcmp(item->d_name, ".")) continue;
        struct entry *entry = &entries[entry_count];
        char path[1280];
        join_path(path, sizeof(path), item->d_name);
        struct stat st;
        if (lstat(path, &st) < 0) continue;
        snprintf(entry->name, sizeof(entry->name), "%s", item->d_name);
        entry->is_dir = S_ISDIR(st.st_mode);
        entry->executable = S_ISREG(st.st_mode) && (st.st_mode & 0111);
        ++entry_count;
    }
    closedir(dir);
    qsort(entries, entry_count, sizeof(entries[0]), compare_entries);
    snprintf(status, sizeof(status), "%zu entries", entry_count);
}

static void go_parent(void) {
    if (!strcmp(cwd, "/")) return;
    char *slash = strrchr(cwd, '/');
    if (slash == cwd) cwd[1] = '\0'; else *slash = '\0';
    load_directory();
}

static void launch(const char *path, int gba) {
    pid_t pid = fork();
    if (pid < 0) { snprintf(status, sizeof(status), "fork failed: %s", strerror(errno)); return; }
    if (!pid) {
        /* Nano-X client sockets are not close-on-exec. Do not let the target
         * inherit WaterFM's protocol connection: after exec it would be an
         * unowned second server client and could corrupt later cleanup. */
        for (int fd = 3; fd < 64; ++fd) close(fd);
        if (gba) execl("/usr/bin/water-mgba", "water-mgba", path, (char *)NULL);
        else execl(path, path, (char *)NULL);
        _exit(127);
    }
    snprintf(status, sizeof(status), "Started %s", path);
}

static int ends_with(const char *text, const char *suffix) {
    size_t a = strlen(text), b = strlen(suffix);
    return a >= b && !strcmp(text + a - b, suffix);
}

static void activate_selected(void) {
    if (selected < 0 || (size_t)selected >= entry_count) return;
    struct entry *entry = &entries[selected];
    if (!strcmp(entry->name, "..")) { go_parent(); return; }
    char path[1280]; join_path(path, sizeof(path), entry->name);
    if (entry->is_dir) { snprintf(cwd, sizeof(cwd), "%s", path); load_directory(); }
    else if (ends_with(entry->name, ".gba")) launch(path, 1);
    else if (entry->executable) launch(path, 0);
    else snprintf(status, sizeof(status), "%s is not launchable", entry->name);
}

static void begin_edit(int rename) {
    if (rename && selected < 0) { snprintf(status, sizeof(status), "Select an entry to rename"); return; }
    editing = 1; edit_rename = rename; edit_len = 0; edit_text[0] = '\0';
    if (rename) { snprintf(edit_text, sizeof(edit_text), "%s", entries[selected].name); edit_len = strlen(edit_text); }
}

static void commit_edit(void) {
    if (!edit_len || strchr(edit_text, '/')) { snprintf(status, sizeof(status), "Name must be non-empty and contain no slash"); editing = 0; return; }
    char path[1280]; join_path(path, sizeof(path), edit_text);
    int result;
    if (edit_rename) {
        char old[1280]; join_path(old, sizeof(old), entries[selected].name);
        result = rename(old, path);
    } else result = mkdir(path, 0755);
    if (result < 0) snprintf(status, sizeof(status), "%s failed: %s", edit_rename ? "Rename" : "Create", strerror(errno));
    else load_directory();
    editing = 0;
}

static void delete_selected(void) {
    if (selected < 0) { snprintf(status, sizeof(status), "Select an entry to delete"); return; }
    if (!strcmp(entries[selected].name, "..")) return;
    char path[1280]; join_path(path, sizeof(path), entries[selected].name);
    int result = entries[selected].is_dir ? rmdir(path) : unlink(path);
    if (result < 0) snprintf(status, sizeof(status), "Delete failed: %s", strerror(errno));
    else load_directory();
}

static void draw(GR_WINDOW_ID window, GR_GC_ID gc) {
    GR_DRAW_ID target = backbuffer;
    GrSetGCForeground(gc, WHITE); GrFillRect(target, gc, 0, 0, WIDTH, HEIGHT);
    GrSetGCForeground(gc, BLACK); GrText(target, gc, 8, 20, cwd, -1, GR_TFTOP);
    GrSetGCForeground(gc, GRAY); GrFillRect(target, gc, 0, TOOLBAR, WIDTH, 1);
    const char *buttons = "[Up]  [New]  [Rename]  [Delete]  [Refresh]";
    GrSetGCForeground(gc, BLACK); GrText(target, gc, 8, TOOLBAR + 20, (void *)buttons, -1, GR_TFTOP);
    size_t rows = visible_rows();
    clamp_scroll();
    for (size_t slot = 0; slot < rows && scroll_first + slot < entry_count; ++slot) {
        size_t i = scroll_first + slot;
        int y = LIST_TOP + (int)slot * ROW;
        if ((int)i == selected) { GrSetGCForeground(gc, MWRGB(180, 210, 255)); GrFillRect(target, gc, 2, y, WIDTH - 4, ROW); }
        char line[300]; snprintf(line, sizeof(line), "%s %s", entries[i].is_dir ? "[DIR]" : (entries[i].executable ? "[EXE]" : "[FILE]"), entries[i].name);
        GrSetGCForeground(gc, BLACK); GrText(target, gc, 10, y + 2, line, -1, GR_TFTOP);
    }
    GrSetGCForeground(gc, MWRGB(225, 225, 225)); GrFillRect(target, gc, WIDTH - SCROLL_WIDTH, LIST_TOP, SCROLL_WIDTH, LIST_BOTTOM - LIST_TOP);
    if (entry_count > rows) {
        int track = LIST_BOTTOM - LIST_TOP;
        int thumb = (int)(track * rows / entry_count);
        if (thumb < 18) thumb = 18;
        int offset = (int)((track - thumb) * scroll_first / max_scroll());
        GrSetGCForeground(gc, GRAY); GrFillRect(target, gc, WIDTH - SCROLL_WIDTH + 2, LIST_TOP + offset, SCROLL_WIDTH - 4, thumb);
    }
    GrSetGCForeground(gc, GRAY); GrFillRect(target, gc, 0, LIST_BOTTOM, WIDTH, 1);
    GrSetGCForeground(gc, BLACK); GrText(target, gc, 8, HEIGHT - 8, editing ? "Name: " : status, -1, GR_TFTOP);
    if (editing) { GrText(target, gc, 58, HEIGHT - 8, edit_text, -1, GR_TFTOP); GrText(target, gc, 58 + (int)edit_len * 8, HEIGHT - 8, "_", 1, GR_TFTOP); }
    GrCopyArea(window, gc, 0, 0, WIDTH, HEIGHT, backbuffer, 0, 0, MWROP_COPY);
}

static void scroll_to_pointer(int y) {
    if (entry_count <= visible_rows()) return;
    int track = LIST_BOTTOM - LIST_TOP;
    int thumb = (int)(track * visible_rows() / entry_count);
    if (thumb < 18) thumb = 18;
    int position = y - LIST_TOP - thumb / 2;
    if (position < 0) position = 0;
    if (position > track - thumb) position = track - thumb;
    scroll_first = (size_t)(position * (int)max_scroll() / (track - thumb));
}

static int click(GR_EVENT_BUTTON *event, struct timespec *last, int *last_index, GR_WINDOW_ID window, GR_GC_ID gc) {
    int x = event->x, y = event->y;
    if (x >= WIDTH - SCROLL_WIDTH && y >= LIST_TOP && y < LIST_BOTTOM) {
        scroll_to_pointer(y); draw(window, gc); return 1;
    }
    if (y >= TOOLBAR && y < TOOLBAR + 30) {
        if (x < 70) go_parent(); else if (x < 140) begin_edit(0); else if (x < 240) begin_edit(1); else if (x < 330) delete_selected(); else load_directory();
        draw(window, gc); return 0;
    }
    int index = (y - LIST_TOP) / ROW;
    if (index < 0 || (size_t)index >= visible_rows() || (size_t)index + scroll_first >= entry_count) return 0;
    index += (int)scroll_first;
    struct timespec now; clock_gettime(CLOCK_MONOTONIC, &now);
    long long delta = (now.tv_sec - last->tv_sec) * 1000LL + (now.tv_nsec - last->tv_nsec) / 1000000LL;
    selected = index;
    if (*last_index == index && delta >= 0 && delta < 450) activate_selected();
    *last = now; *last_index = index; draw(window, gc); return 0;
}

static void key(GR_EVENT_KEYSTROKE *event, GR_WINDOW_ID window, GR_GC_ID gc) {
    int ch = event->ch;
    if (editing) {
        if (ch == MWKEY_ENTER) commit_edit();
        else if (ch == MWKEY_ESCAPE) editing = 0;
        else if (ch == MWKEY_BACKSPACE && edit_len) edit_text[--edit_len] = '\0';
        else if (ch >= 32 && ch < 127 && edit_len < NAME_MAX_LEN) { edit_text[edit_len++] = (char)ch; edit_text[edit_len] = '\0'; }
    } else if (ch == MWKEY_F2) begin_edit(0);
    else if (ch == MWKEY_DELETE) delete_selected();
    else if (ch == MWKEY_BACKSPACE) go_parent();
    else if (ch == MWKEY_ENTER) activate_selected();
    draw(window, gc);
}

int main(int argc, char **argv) {
    if (argc == 2) {
        struct stat st;
        if (argv[1][0] != '/' || stat(argv[1], &st) < 0 || !S_ISDIR(st.st_mode)) {
            fprintf(stderr, "waterfm: expected an existing absolute directory: %s\n", argv[1]);
            return 1;
        }
        snprintf(cwd, sizeof(cwd), "%s", argv[1]);
        size_t length = strlen(cwd);
        while (length > 1 && cwd[length - 1] == '/') cwd[--length] = '\0';
    }
    load_directory();
    if (GrOpen() < 0) { fprintf(stderr, "waterfm: Nano-X is not running\n"); return 1; }
    GR_WINDOW_ID window = GrNewWindowEx(GR_WM_PROPS_APPWINDOW, "WaterOS File Manager",
                                         GR_ROOT_WINDOW_ID, 40, 40, WIDTH, HEIGHT, WHITE);
    GR_GC_ID gc = GrNewGC();
    backbuffer = GrNewPixmap(WIDTH, HEIGHT, NULL);
    if (!gc || !backbuffer) {
        fprintf(stderr, "waterfm: cannot create Nano-X drawing resources\n");
        if (backbuffer) GrDestroyWindow(backbuffer);
        if (gc) GrDestroyGC(gc);
        GrDestroyWindow(window); GrClose(); return 1;
    }
    GrSetGCUseBackground(gc, GR_FALSE);
    GrSelectEvents(window, GR_EVENT_MASK_EXPOSURE | GR_EVENT_MASK_BUTTON_DOWN | GR_EVENT_MASK_BUTTON_UP |
                   GR_EVENT_MASK_MOUSE_POSITION | GR_EVENT_MASK_KEY_DOWN | GR_EVENT_MASK_CLOSE_REQ);
    GrMapWindow(window);
    /* WaterOS' small window-manager path does not guarantee an immediate
     * exposure event after mapping, so paint the initial directory explicitly. */
    draw(window, gc);
    struct timespec last = {0}; int last_index = -1; int dragging_scrollbar = 0; int running = 1;
    while (running) { GR_EVENT event; GrGetNextEvent(&event); switch (event.type) {
        case GR_EVENT_TYPE_EXPOSURE: draw(window, gc); break;
        case GR_EVENT_TYPE_BUTTON_DOWN: if (!editing) dragging_scrollbar = click(&event.button, &last, &last_index, window, gc); break;
        case GR_EVENT_TYPE_BUTTON_UP: dragging_scrollbar = 0; break;
        case GR_EVENT_TYPE_MOUSE_POSITION:
            if (dragging_scrollbar) { scroll_to_pointer(event.mouse.y); draw(window, gc); }
            break;
        case GR_EVENT_TYPE_KEY_DOWN: key(&event.keystroke, window, gc); break;
        case GR_EVENT_TYPE_CLOSE_REQ: running = 0; break;
    }}
    GrDestroyWindow(backbuffer); GrDestroyGC(gc); GrDestroyWindow(window); GrClose(); return 0;
}
