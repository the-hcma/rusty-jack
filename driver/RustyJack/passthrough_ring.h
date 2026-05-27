#ifndef RUSTY_JACK_PASSTHROUGH_RING_H
#define RUSTY_JACK_PASSTHROUGH_RING_H

#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>

#define RJ_PASSTHROUGH_RING_MAGIC 0x54504A52u
#define RJ_PASSTHROUGH_RING_VERSION 1u
#define RJ_PASSTHROUGH_RING_SLOT_COUNT 16u
#define RJ_PASSTHROUGH_RING_FRAMES 512u
#define RJ_PASSTHROUGH_RING_CHANNELS 2u
#define RJ_PASSTHROUGH_RING_SAMPLES (RJ_PASSTHROUGH_RING_FRAMES * RJ_PASSTHROUGH_RING_CHANNELS)

/* Shared with rusty-jack daemon (`passthrough::PASSTHROUGH_RING_PATH`). */
#define RJ_PASSTHROUGH_SHARED_DIR "/Library/Application Support/rusty-jack"
#define RJ_PASSTHROUGH_RING_PATH RJ_PASSTHROUGH_SHARED_DIR "/passthrough.ring"

typedef struct {
    atomic_uint_least64_t write_index;
    atomic_uint_least64_t read_index;
    uint32_t magic;
    uint32_t version;
    float volume_scalar;
    uint32_t muted;
    uint32_t sample_rate_hz;
    uint32_t frame_size;
    uint32_t channel_count;
    uint32_t reserved;
} rj_passthrough_header_t;

typedef struct {
    atomic_uint_least64_t seq;
    uint32_t frame_count;
    uint32_t reserved;
    float samples[RJ_PASSTHROUGH_RING_SAMPLES];
} rj_passthrough_slot_t;

typedef struct {
    rj_passthrough_header_t header;
    rj_passthrough_slot_t slots[RJ_PASSTHROUGH_RING_SLOT_COUNT];
} rj_passthrough_ring_t;

bool rj_passthrough_ring_open(rj_passthrough_ring_t **out_ring);
void rj_passthrough_ring_close(rj_passthrough_ring_t *ring);
void rj_passthrough_ring_push_write_mix(
    rj_passthrough_ring_t *ring,
    const float *samples,
    uint32_t frame_count,
    float volume_scalar,
    uint32_t muted);

#endif
