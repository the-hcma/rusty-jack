# Rusty Jack

macOS CLI that routes system audio to your chosen **HDMI, DisplayPort, USB-C dock, or line-out** output using JSON policy, an interactive picker, and a launchd-friendly daemon. For fixed-volume HDMI/DisplayPort outputs, Rusty Jack currently uses [eqMac](https://eqmac.app) as the software volume layer when it is installed.

> *Your preferred output, on deck — without a menu bar app.*

## Quick start

```bash
brew tap thehcma/tap
brew install rusty-jack

rusty-jack list
rusty-jack install   # pick preferred + fallback outputs; starts the daemon
rusty-jack status
```

For **HDMI/DP volume keys**, install [eqMac](https://eqmac.app) — rusty-jack will start it automatically when needed and warn with the download URL when it is missing. See [Volume on external displays](#volume-on-external-displays).

Full command reference: [docs/USAGE.md](./docs/USAGE.md). Troubleshooting: [docs/TROUBLESHOOTING.md](./docs/TROUBLESHOOTING.md).

---

## The problem

When macOS plays through an external display (DisplayPort / HDMI / many docks), **F10 / F11 / F12 often don’t control audible volume**. The monitor is usually a **fixed-gain digital output**: macOS can route audio to it, but there is nothing meaningful for the system volume slider or keyboard keys to adjust.

Built-in speakers and most Bluetooth headsets expose **software-controllable volume** in CoreAudio. External displays typically **do not**.

## Current Capabilities

| Capability | Command / config |
|------------|------------------|
| List output devices with transport, monitor name, active route, and routability | `list`, `list --hdmi` |
| Show current route, policy match, system virtual default, and volume | `status` |
| Switch once to preferred device or fallback from config | `apply` |
| Pick interactively or by device index | `picker`, `picker --index N` |
| Apply configured volume after a real switch | `volume` |
| Start eqMac for HDMI-class routes when available; warn when missing | automatic during `apply` / `picker` / `daemon` |
| Run a background auto-switch supervisor | `daemon` |
| Pause, resume, or uninstall the per-user LaunchAgent | `pause`, `resume`, `disable` |
| Wake Sony Songpal / ScalarWebAPI speakers on output selection or idle-to-active daemon triggers | `sony_speaker` |

Switching the default output to a **physical HDMI device alone does not fix volume keys**. Until Rusty Jack has its own virtual HAL driver, use **eqMac** as the software volume layer.

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
| Not installed | Warning on stderr with https://eqmac.app; HDMI volume buttons will not work |

Detection: `/Applications/eqMac.app` or `/Library/Audio/Plug-Ins/HAL/eqMac.driver`, process via `pgrep -x eqMac`. If a stale driver remains but the app cannot be launched, Rusty Jack treats eqMac as not installed and still switches routes.

### Config `volume`

When set (0–100), rusty-jack applies it **only on an actual device switch** (`apply` or `picker` when picking the configured preferred device). Setting uses device scalar + system volume, with **retries** so eqMac cannot silently reset the level right after a route change.

### Planned native driver

[IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md) tracks the future Rusty Jack virtual AudioServerPlugIn + daemon passthrough work. That is the path to removing the eqMac dependency for HDMI/DP volume keys.

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
| `apply` | Switch to preferred/fallback from config |
| `daemon` | Long-running policy loop for launchd |
| `disable` | Uninstall launchd agent (remove plist) |
| `install` | Install and start the per-user LaunchAgent |
| `list` | Table of output devices (`--hdmi`, `--json`) |
| `pause` | Stop launchd agent; keep plist |
| `picker` | Interactive menu or `--index N` to switch |
| `resume` | Re-enable launchd agent |
| `status` | Devices + virtual default block + policy + volume + daemon state |
| `uninstall` | Uninstall launchd agent (alias for `disable`) |
| `upgrade` | Refresh LaunchAgent to current binary and restart it |

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
| `auto_switch` | Master enable for the daemon loop |
| `poll_interval_ms` | Daemon route check interval (default `3000`) |
| `switch_delay_ms` | Delay after daemon switch before wake hooks (default `500`) |
| `activity_idle_threshold_ms` | Idle time that counts as away before an idle-to-active trigger (default `60000`) |
| `activity_poll_interval_ms` | Daemon idle-state sampling interval (default `1000`) |
| `sony_speaker` | Wake SRS-ZR5 on `apply`, `picker`, daemon output switches, and daemon idle-to-active triggers using discovered ScalarWebAPI endpoint |

Minimal example:

```json
{
  "version": 1,
  "preferred_device": { "monitor_name": "DELL U3219Q" },
  "fallback_uids": [],
  "also_set_system_output": true,
  "volume": 13
}
```

`match`, `exclude`, and `logging` in `config.example.json` are reserved for future behavior and currently ignored by the loader.

Sony ZR5 example: [`config.example.sony.json`](./config.example.sony.json). Other Sony Songpal / ScalarWebAPI speakers may also work; Rusty Jack discovers the advertised ScalarWebAPI endpoint and uses `system.getPowerStatus` / `system.setPowerStatus`. If you confirm another model, please consider contributing device info to [python-songpal on GitHub](https://github.com/rytilahti/python-songpal) using the [`python-songpal` PyPI package](https://pypi.org/project/python-songpal/).

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
| `disable` | Stop, disable, **delete plist** |
| `install` | Write plist for the current binary, enable, and start |
| `pause` | `bootout` + `disable`; plist **kept** |
| `resume` | `enable` + `bootstrap` |
| `uninstall` | Same daemon uninstall behavior as `disable` |
| `upgrade` | Rewrite plist for the current binary path and restart |

`daemon` runs in the current user session and reloads config before each scheduled poll. The LaunchAgent is per-user: each macOS login account that wants auto-routing installs its own plist under `~/Library/LaunchAgents`, with its own config and logs. Two users can install it at the same time because each job lives in a separate `gui/<uid>` launchd domain.

### Install the LaunchAgent

```bash
make install
rusty-jack install
```

`install` creates `~/.config/rusty-jack/config.json` when needed, prompting for a preferred output and a fallback output (defaulting the fallback to the Mac's built-in speakers when available). It then renders `~/Library/LaunchAgents/com.example.rusty-jack.plist` from the bundled template, points it at the current `rusty-jack` binary, creates `~/Library/Logs`, and bootstraps the job in your `gui/<uid>` launchd domain.

Use `rusty-jack pause` to stop auto-routing temporarily, `rusty-jack resume` to start it again, and `rusty-jack uninstall` to stop it and remove the plist. Uninstall offers to remove `~/.config/rusty-jack/config.json`; use `disable` for daemon-only removal that always keeps config and logs.

### Update the daemon

When a new Rusty Jack version is available, stop the running job before replacing the binary, then start it again:

```bash
rusty-jack pause
git pull
make upgrade
```

`make upgrade` installs the new binary once, then runs `rusty-jack upgrade` to refresh and restart the LaunchAgent. The CLI `upgrade` command itself does not download or build Rusty Jack.

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
make upgrade    # install once, then restart LaunchAgent
make uninstall  # stop/remove LaunchAgent, then cargo uninstall rusty-jack
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

`build`, `release`, `test`, `fmt`, `clippy`, `universal`, `install`, `upgrade`, `uninstall`, `clean` — see [Makefile](Makefile).

---

## Troubleshooting

See **[docs/TROUBLESHOOTING.md](./docs/TROUBLESHOOTING.md)** for:

- Volume keys dead on HDMI without eqMac
- eqMac installed but volume still wrong
- Zoom / virtual devices in the picker
- Policy “no change” / wrong monitor

---

## Roadmap

| Area | Status |
|------|--------|
| Routing CLI, eqMac integration, volume retries, daemon polling | Implemented |
| Sony speaker wake via ScalarWebAPI + daemon idle polling | Implemented |
| LaunchAgent install, upgrade, uninstall, and status helper | Implemented |
| Native event listener refinements for activity detection | Planned |
| Own virtual driver + software volume | Planned |

Full plan: **[IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md)**.

---

## Packaging

Rusty Jack is distributed through the personal Homebrew tap `thehcma/tap`:

```bash
brew tap thehcma/tap
brew install rusty-jack
```

The formula source lives at [`packaging/homebrew/rusty-jack.rb`](./packaging/homebrew/rusty-jack.rb).

---

## License

Copyright (c) 2026 Henrique Andrade / thehcma.

MIT
