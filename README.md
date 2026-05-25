# Rusty Jack

macOS CLI that routes system audio to your chosen **HDMI, DisplayPort, or USB-C dock** output and keeps **volume keys working** on external displays — the same core problem [eqMac](https://github.com/bitgapp/eqMac) solves, as a **headless, launchd-friendly** tool with JSON config. No menu bar, no EQ.

> *Your HDMI output, on deck — and the volume keys actually work.*

## Quick start

```bash
# Prerequisites: macOS 12+, Xcode CLT, Rust 1.85+
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

git clone https://github.com/thehcma/rusty-jack.git
cd rusty-jack
make release

mkdir -p ~/.config/rusty-jack
cp config.example.json ~/.config/rusty-jack/config.json
# Edit preferred_device.monitor_name to match `rusty-jack list`

./target/release/rusty-jack list
./target/release/rusty-jack status
./target/release/rusty-jack apply
```

For **HDMI/DP volume**, install [eqMac](https://eqmac.app) — rusty-jack will start it automatically when needed (until a built-in virtual driver ships). See [Volume on external displays](#volume-on-external-displays).

Full command reference: [docs/USAGE.md](./docs/USAGE.md). Troubleshooting: [docs/TROUBLESHOOTING.md](./docs/TROUBLESHOOTING.md).

---

## The problem

When macOS plays through an external display (DisplayPort / HDMI / many docks), **F10 / F11 / F12 often don’t control audible volume**. The monitor is usually a **fixed-gain digital output**: macOS can route audio to it, but there is nothing meaningful for the system volume slider or keyboard keys to adjust.

Built-in speakers and most Bluetooth headsets expose **software-controllable volume** in CoreAudio. External displays typically **do not**.

## What Rusty Jack does today

| Feature | Status |
|---------|--------|
| List output devices (transport, monitor name, active `>`) | **Done** — `list`, `list --hdmi` |
| Policy + routing status | **Done** — `status` |
| Switch to preferred/fallback from config | **Done** — `apply` |
| Interactive / scripted device pick | **Done** — `picker`, `picker --index N` |
| Config volume on switch (with retries) | **Done** — `volume` in config |
| eqMac auto-start for HDMI routes | **Done** — see below |
| launchd pause / resume / disable | **Done** — `pause`, `resume`, `disable` |
| Background auto-switch daemon | **Planned** — `daemon` subcommand exists but not implemented |
| Own virtual HAL driver (replace eqMac) | **Planned** — [IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md) Phase 7 |

Switching the default output to a **physical HDMI device alone does not fix volume keys**. Until Phase 7, use **eqMac** (or similar) as the software volume layer.

---

## Volume on external displays

### Why HDMI volume is hard

Physical HDMI/DP endpoints often have **no settable CoreAudio volume scalar**. macOS sends a fixed digital stream; keyboard volume has nothing to drive.

### Interim solution: eqMac

eqMac provides:

1. A **virtual HAL device** as the system default (what volume keys target).
2. An **app** that captures audio, applies software gain, and renders to the physical monitor.

Rusty Jack **does not replace eqMac yet** — it **routes** to the right monitor and **starts eqMac** when you switch to an HDMI-class device:

| eqMac state | Behavior |
|-------------|----------|
| Installed + running | No action |
| Installed, not running | `open -a eqMac`, brief startup wait |
| Not installed | Warning on stderr; volume keys on HDMI may not work |

Detection: `/Applications/eqMac.app` or `/Library/Audio/Plug-Ins/HAL/eqMac.driver`, process via `pgrep -x eqMac`.

### Config `volume`

When set (0–100), rusty-jack applies it **only on an actual device switch** (`apply` or `picker` when picking the configured preferred device). Setting uses device scalar + system volume, with **retries** so eqMac cannot silently reset the level right after a route change.

### Future: native driver

[IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md) **Phase 7** — Rusty Jack virtual AudioServerPlugIn + daemon passthrough, no eqMac dependency.

---

## Platform

- **macOS 12 Monterey** or later (Intel and Apple Silicon)
- **macOS only** — CoreAudio; not built for Linux
- **Rust 1.85+** (`rust-version` in `Cargo.toml`)
- CI: **`macos-14`** — rustfmt, clippy, tests, release builds ([`.github/workflows/ci.yml`](.github/workflows/ci.yml))

---

## Build (local)

Requires a Mac. See [Build (local — debug & release)](#build-local--debug--release) below or [docs/USAGE.md § Build](./docs/USAGE.md#build).

```bash
make release          # → target/release/rusty-jack
./target/release/rusty-jack --help   # version + git commit
make test
```

---

## Commands (summary)

Global flag: `--config PATH` (overrides `RUSTY_JACK_CONFIG` and `~/.config/rusty-jack/config.json`).

| Command | Purpose |
|---------|---------|
| `list` | Table of output devices (`--hdmi`, `--json`) |
| `status` | Devices + virtual default block + policy + volume |
| `apply` | Switch to preferred/fallback from config |
| `picker` | Interactive menu or `--index N` to switch |
| `pause` | Stop launchd agent; keep plist |
| `resume` | Re-enable launchd agent |
| `disable` | Uninstall launchd agent (remove plist) |
| `daemon` | *Not implemented* — reserved for background loop |

All subcommands support `--json` where applicable. Subcommands are alphabetical in `--help`.

Details, JSON shapes, and picker legend: **[docs/USAGE.md](./docs/USAGE.md)**.

---

## Configuration

Default path: `~/.config/rusty-jack/config.json`. Copy from [`config.example.json`](./config.example.json).

### Implemented fields

| Field | Description |
|-------|-------------|
| `version` | Must be `1` |
| `preferred_device.monitor_name` | Match display product name from `list` (unique) |
| `preferred_device.uid` | Or match CoreAudio UID directly |
| `preferred_device_uid` | Legacy; use `preferred_device.uid` |
| `fallback_uids` | Try in order if preferred is unplugged |
| `also_set_system_output` | Also set system/alert output (default `true`) |
| `volume` | 0–100; apply on switch to preferred only |
| `auto_switch` | For future daemon (ignored by CLI today) |
| `sony_speaker` | Phase 8 — wake SRS-ZR5 on activity (config validates; not wired to daemon yet) |

Example:

```json
{
  "version": 1,
  "preferred_device": { "monitor_name": "DELL U3219Q" },
  "fallback_uids": [],
  "also_set_system_output": true,
  "volume": 13
}
```

`config.example.json` may include extra keys (`poll_interval_ms`, `match`, `exclude`, …) reserved for future daemon behavior; they are **ignored** by the current loader.

Sony ZR5 example: [`config.example.sony.json`](./config.example.sony.json).

---

## Picker and device list

### Active vs preferred vs dim

| Marker | Meaning |
|--------|---------|
| `>` (green) | Currently active physical route |
| `*` (cyan) | Config preferred device |
| dim | Not routable (e.g. Zoom virtual, aggregates) |

**ZoomAudioDevice** and similar app virtual devices are shown but **cannot be selected** — they are not speaker outputs.

### eqMac in `list` / `status`

When eqMac is the HAL default, the physical monitor appears with `>` on its row; a **System default (virtual)** footer describes the eqMac router and routed-to monitor.

---

## launchd (daemon control)

LaunchAgent label: `com.example.rusty-jack`  
Plist template: [`launchd/com.example.rusty-jack.plist.template`](./launchd/com.example.rusty-jack.plist.template)

| Command | Effect |
|---------|--------|
| `pause` | `bootout` + `disable`; plist **kept** |
| `resume` | `enable` + `bootstrap` |
| `disable` | Stop, disable, **delete plist** |

The background `daemon` loop is not implemented yet; install the plist only when that lands.

---

## Build (local — debug & release)

Requires **macOS 12+**, **Apple Silicon or Intel**, and **[Rust](https://rustup.rs/) 1.85+**.

### 1. One-time setup

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version
```

### 2. Clone and build

```bash
git clone https://github.com/thehcma/rusty-jack.git
cd rusty-jack
```

| Build | Command | Output |
|-------|---------|--------|
| Debug | `make build` | `target/debug/rusty-jack` |
| Release | `make release` | `target/release/rusty-jack` |

```bash
make release
./target/release/rusty-jack --help   # e.g. rusty-jack 0.1.0 (commit 7855685)
```

### 3. Install to PATH (optional)

```bash
make install    # ~/.cargo/bin/rusty-jack
```

### 4. Universal binary

```bash
make universal   # target/release/rusty-jack-universal
```

### 5. Verify on another Mac

```bash
make test
./target/release/rusty-jack list
./target/release/rusty-jack status
./target/release/rusty-jack apply
./target/release/rusty-jack picker --index 0 --json
```

Confirm `--help` commit matches `git rev-parse --short HEAD`.

### Makefile targets

`build`, `release`, `test`, `fmt`, `clippy`, `universal`, `install`, `clean` — see [Makefile](Makefile).

---

## Troubleshooting

See **[docs/TROUBLESHOOTING.md](./docs/TROUBLESHOOTING.md)** for:

- Volume keys dead on HDMI without eqMac
- eqMac installed but volume still wrong
- Zoom / virtual devices in the picker
- Policy “no change” / wrong monitor

---

## Roadmap

| Phase | Work |
|-------|------|
| **Now** | Routing CLI, eqMac integration, volume retries |
| **Next** | `daemon` + launchd auto-switch |
| **Phase 7** | Own virtual driver + software volume |
| **Phase 8** | Sony SRS-ZR5 wake on keyboard/mouse activity |

Full plan: **[IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md)**.

---

## Install via Homebrew (planned)

```bash
brew install rusty-jack   # from your tap
rusty-jack agent install  # when agent install ships
```

Formula sketch: [`packaging/homebrew/rusty-jack.rb`](./packaging/homebrew/rusty-jack.rb).

---

## License

MIT
