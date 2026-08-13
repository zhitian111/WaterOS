/* Minimal mGBA 0.10.5 frontend for WaterOS Nano-X. Audio is intentionally off. */
#include <nano-X.h>

#include <mgba/core/core.h>
#include <mgba/core/log.h>
#include <mgba/internal/gba/input.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <time.h>

enum { GBA_WIDTH = 240, GBA_HEIGHT = 160 };

static uint32_t video[GBA_WIDTH * GBA_HEIGHT];

/* The core's default stderr logger reports normal BIOS/DMA activity every
 * frame. Keep the serial console usable; frontend failures are reported
 * explicitly below. */
static void discard_core_log(struct mLogger *logger, int category,
                             enum mLogLevel level, const char *format,
                             va_list args) {
    (void) logger;
    (void) category;
    (void) level;
    (void) format;
    (void) args;
}

static struct mLogger core_logger = { .log = discard_core_log };

/* mGBA's default 32-bit buffer is X-B-G-R (0x00BBGGRR), whereas Nano-X
 * MWPF_TRUECOLORARGB consumes 0xAARRGGBB. Do the conversion explicitly so
 * the result does not depend on host endianness or private renderer details. */
static void convert_video(uint32_t *present) {
    for (size_t source_y = 0; source_y < GBA_HEIGHT; ++source_y) {
        for (size_t source_x = 0; source_x < GBA_WIDTH; ++source_x) {
            uint32_t pixel = video[source_y * GBA_WIDTH + source_x];
            uint32_t argb = 0xFF000000u |
                            ((pixel & 0x000000FFu) << 16) |
                            (pixel & 0x0000FF00u) |
                            ((pixel & 0x00FF0000u) >> 16);
            present[source_y * GBA_WIDTH + source_x] = argb;
        }
    }
}

static void update_fps_title(GR_WINDOW_ID window, unsigned *frames,
                             struct timespec *started, unsigned scale) {
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    long long elapsed_ns = (long long) (now.tv_sec - started->tv_sec) * 1000000000LL +
                           (long long) now.tv_nsec - started->tv_nsec;
    if (elapsed_ns < 1000000000LL) return;
    unsigned fps = (unsigned) ((*frames * 1000000000LL) / elapsed_ns);
    char title[64];
    snprintf(title, sizeof(title), "WaterOS mGBA %ux - %u FPS", scale, fps);
    GrSetWindowTitle(window, title);
    fprintf(stderr, "[water-mgba] %u FPS\n", fps);
    *frames = 0;
    *started = now;
}

static int parse_arguments(int argc, char **argv, unsigned *scale, const char **rom) {
    *scale = 2;
    if (argc == 2) {
        *rom = argv[1];
        return 0;
    }
    if (argc == 4 && (!strcmp(argv[1], "--scale") || !strcmp(argv[1], "-s"))) {
        char *end = NULL;
        unsigned long parsed = strtoul(argv[2], &end, 10);
        if (*argv[2] && end && !*end && parsed >= 1 && parsed <= 4) {
            *scale = (unsigned) parsed;
            *rom = argv[3];
            return 0;
        }
        fprintf(stderr, "water-mgba: scale must be an integer from 1 to 4\n");
        return -1;
    }
    fprintf(stderr, "usage: %s [--scale 1..4] ROM.gba\n", argv[0]);
    return -1;
}

static uint32_t key_for(int ch) {
    switch (ch) {
    case MWKEY_UP: return 1u << GBA_KEY_UP;
    case MWKEY_DOWN: return 1u << GBA_KEY_DOWN;
    case MWKEY_LEFT: return 1u << GBA_KEY_LEFT;
    case MWKEY_RIGHT: return 1u << GBA_KEY_RIGHT;
    case 'x': case 'X': return 1u << GBA_KEY_A;
    case 'z': case 'Z': return 1u << GBA_KEY_B;
    case 'a': case 'A': return 1u << GBA_KEY_L;
    case 's': case 'S': return 1u << GBA_KEY_R;
    case MWKEY_ENTER: return 1u << GBA_KEY_START;
    case MWKEY_BACKSPACE: return 1u << GBA_KEY_SELECT;
    default: return 0;
    }
}

static void handle_events(struct mCore *core, int *running) {
    GR_EVENT event;
    for (;;) {
        GrCheckNextEvent(&event);
        if (event.type == GR_EVENT_TYPE_NONE) return;
        if (event.type == GR_EVENT_TYPE_CLOSE_REQ) { *running = 0; return; }
        if (event.type == GR_EVENT_TYPE_KEY_DOWN || event.type == GR_EVENT_TYPE_KEY_UP) {
            uint32_t key = key_for(((GR_EVENT_KEYSTROKE *) &event)->ch);
            if (key) {
                if (event.type == GR_EVENT_TYPE_KEY_DOWN) core->addKeys(core, key);
                else core->clearKeys(core, key);
            }
        }
    }
}

static void pace(struct timespec *deadline) {
    const long frame_ns = 16742706L; /* 1 / 59.7275 Hz */
    deadline->tv_nsec += frame_ns;
    if (deadline->tv_nsec >= 1000000000L) { deadline->tv_sec++; deadline->tv_nsec -= 1000000000L; }
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    if (now.tv_sec < deadline->tv_sec ||
        (now.tv_sec == deadline->tv_sec && now.tv_nsec < deadline->tv_nsec)) {
        clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, deadline, NULL);
    } else {
        *deadline = now;
    }
}

int main(int argc, char **argv) {
    unsigned scale;
    const char *rom;
    if (parse_arguments(argc, argv, &scale, &rom) != 0) return 2;
    size_t present_width = GBA_WIDTH * scale;
    size_t present_height = GBA_HEIGHT * scale;
    uint32_t *present = calloc(GBA_WIDTH * GBA_HEIGHT, sizeof(*present));
    if (!present) { fprintf(stderr, "water-mgba: cannot allocate video buffer\n"); return 1; }
    mLogSetDefaultLogger(&core_logger);
    struct mCore *core = mCoreFind(rom);
    if (!core) { fprintf(stderr, "water-mgba: unsupported ROM: %s\n", rom); free(present); return 1; }
    core->init(core);
    mCoreInitConfig(core, "wateros");
    if (!mCoreLoadFile(core, rom)) { fprintf(stderr, "water-mgba: cannot load %s\n", rom); free(present); return 1; }
    core->setVideoBuffer(core, (color_t *) video, GBA_WIDTH);
    core->reset(core);
    if (GrOpen() < 0) { fprintf(stderr, "water-mgba: Nano-X is not running\n"); return 1; }
    GR_WINDOW_ID window = GrNewWindowEx(GR_WM_PROPS_APPWINDOW, "WaterOS mGBA",
        GR_ROOT_WINDOW_ID, 10, 10, present_width, present_height, BLACK);
    GR_GC_ID gc = GrNewGC();
    /* Keep the source image native-sized. GrStretchArea performs nearest-neighbor
     * expansion in Nano-X, avoiding a fourfold user-space buffer and upload. */
    GR_WINDOW_ID pixmap = GrNewPixmap(GBA_WIDTH, GBA_HEIGHT, NULL);
    if (!gc || !pixmap) {
        fprintf(stderr, "water-mgba: cannot create Nano-X drawing resources\n");
        if (pixmap) GrDestroyWindow(pixmap);
        if (gc) GrDestroyGC(gc);
        GrDestroyWindow(window);
        GrClose();
        core->unloadROM(core);
        mCoreConfigDeinit(&core->config);
        core->deinit(core);
        free(present);
        return 1;
    }
    GrSelectEvents(window, GR_EVENT_MASK_CLOSE_REQ | GR_EVENT_MASK_KEY_DOWN | GR_EVENT_MASK_KEY_UP);
    GrMapWindow(window);
    struct timespec deadline;
    clock_gettime(CLOCK_MONOTONIC, &deadline);
    struct timespec fps_started = deadline;
    unsigned fps_frames = 0;
    int running = 1;
    while (running) {
        handle_events(core, &running);
        if (!running) break;
        core->runFrame(core);
        convert_video(present);
        GrArea(pixmap, gc, 0, 0, GBA_WIDTH, GBA_HEIGHT, present, MWPF_TRUECOLORARGB);
        GrStretchArea(window, gc, 0, 0, present_width, present_height,
                      pixmap, 0, 0, GBA_WIDTH, GBA_HEIGHT, MWROP_COPY);
        ++fps_frames;
        update_fps_title(window, &fps_frames, &fps_started, scale);
        pace(&deadline);
    }
    GrDestroyWindow(pixmap);
    GrDestroyGC(gc);
    GrDestroyWindow(window);
    GrClose();
    core->unloadROM(core);
    mCoreConfigDeinit(&core->config);
    core->deinit(core);
    free(present);
    return 0;
}
