#include "passthrough_ring.h"

#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <limits.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

static const char *ring_path(void) {
    return RJ_PASSTHROUGH_RING_PATH;
}

static bool ensure_parent_dir(const char *file_path) {
    char dir[PATH_MAX];
    strncpy(dir, file_path, sizeof(dir));
    dir[sizeof(dir) - 1] = '\0';
    char *slash = strrchr(dir, '/');
    if (slash == NULL) {
        return false;
    }
    *slash = '\0';
    for (char *cursor = dir + 1; *cursor != '\0'; cursor++) {
        if (*cursor != '/') {
            continue;
        }
        *cursor = '\0';
        (void)mkdir(dir, 0755);
        *cursor = '/';
    }
    if (mkdir(dir, 0777) != 0 && errno != EEXIST) {
        return false;
    }
    (void)chmod(dir, 0777);
    return true;
}

bool rj_passthrough_ring_open(rj_passthrough_ring_t **out_ring) {
    if (out_ring == NULL) {
        return false;
    }
    *out_ring = NULL;

    const char *path = ring_path();
    if (path == NULL) {
        return false;
    }
    if (!ensure_parent_dir(path)) {
        return false;
    }

    const size_t map_size = sizeof(rj_passthrough_ring_t);
    int fd = open(path, O_RDWR | O_CREAT, 0644);
    if (fd < 0) {
        return false;
    }
    if (ftruncate(fd, (off_t)map_size) != 0) {
        close(fd);
        return false;
    }

    void *mapped = mmap(NULL, map_size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    close(fd);
    if (mapped == MAP_FAILED) {
        return false;
    }

    rj_passthrough_ring_t *ring = (rj_passthrough_ring_t *)mapped;
    uint32_t magic = ring->header.magic;
    if (magic != RJ_PASSTHROUGH_RING_MAGIC) {
        memset(ring, 0, map_size);
        ring->header.magic = RJ_PASSTHROUGH_RING_MAGIC;
        ring->header.version = RJ_PASSTHROUGH_RING_VERSION;
        ring->header.sample_rate_hz = 48000;
        ring->header.frame_size = RJ_PASSTHROUGH_RING_FRAMES;
        ring->header.channel_count = RJ_PASSTHROUGH_RING_CHANNELS;
        ring->header.volume_scalar = 1.0f;
    }

    *out_ring = ring;
    return true;
}

void rj_passthrough_ring_close(rj_passthrough_ring_t *ring) {
    if (ring == NULL) {
        return;
    }
    (void)munmap(ring, sizeof(rj_passthrough_ring_t));
}

void rj_passthrough_ring_push_write_mix(
    rj_passthrough_ring_t *ring,
    const float *samples,
    uint32_t frame_count,
    float volume_scalar,
    uint32_t muted) {
    if (ring == NULL || samples == NULL || frame_count == 0) {
        return;
    }
    if (frame_count > RJ_PASSTHROUGH_RING_FRAMES) {
        frame_count = RJ_PASSTHROUGH_RING_FRAMES;
    }

    ring->header.volume_scalar = volume_scalar;
    ring->header.muted = muted;

    uint64_t index = atomic_fetch_add_explicit(&ring->header.write_index, 1, memory_order_relaxed);
    rj_passthrough_slot_t *slot = &ring->slots[index % RJ_PASSTHROUGH_RING_SLOT_COUNT];
    const uint32_t sample_count = frame_count * RJ_PASSTHROUGH_RING_CHANNELS;
    const float gain = muted ? 0.0f : volume_scalar;

    for (uint32_t i = 0; i < sample_count; i++) {
        slot->samples[i] = samples[i] * gain;
    }
    if (sample_count < RJ_PASSTHROUGH_RING_SAMPLES) {
        memset(slot->samples + sample_count, 0, (RJ_PASSTHROUGH_RING_SAMPLES - sample_count) * sizeof(float));
    }
    slot->frame_count = frame_count;
    atomic_store_explicit(&slot->seq, index + 1, memory_order_release);
}
