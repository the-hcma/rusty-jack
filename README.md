# Rusty Jack

[![CI](https://github.com/the-hcma/rusty-jack/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/the-hcma/rusty-jack/actions/workflows/ci.yml)
[![Release Please](https://github.com/the-hcma/rusty-jack/actions/workflows/release-please.yml/badge.svg?branch=main)](https://github.com/the-hcma/rusty-jack/actions/workflows/release-please.yml)
[![Release](https://github.com/the-hcma/rusty-jack/actions/workflows/release.yml/badge.svg)](https://github.com/the-hcma/rusty-jack/actions/workflows/release.yml)
[![GitHub release](https://img.shields.io/github/v/release/the-hcma/rusty-jack?sort=semver)](https://github.com/the-hcma/rusty-jack/releases)
[![Homebrew tap](https://img.shields.io/badge/homebrew-the--hcma%2Ftap-blue?logo=homebrew)](https://github.com/the-hcma/homebrew-tap)

macOS CLI that keeps audio on your chosen **HDMI, DisplayPort, USB-C dock, or line-out** output, helps keyboard volume keys work with fixed-volume HDMI/DisplayPort outputs, and wakes ScalarWebAPI-compatible speakers when their Mac output is selected.

> *Your preferred output, on deck — without a menu bar app.*

## Quick start

Install Rusty Jack with Homebrew:

```bash
brew tap the-hcma/tap
brew install rusty-jack
```

Then choose the preferred output, optionally choose an explicit fallback, and start the per-user daemon:

```bash
rusty-jack list
rusty-jack install   # pick preferred + optional fallback outputs; starts the daemon
rusty-jack status
```

If `~/.config/rusty-jack/config.json` already exists, `install` preserves it and migrates it in place. It updates readable device `name` labels for known UIDs and offers additive choices, without dropping custom settings such as `scalar_webapi_device`.

For **HDMI/DP volume keys**, Rusty Jack offers its native audio driver when a connected HDMI/DisplayPort output is present. If [eqMac](https://eqmac.app) is already installed, Rusty Jack can use it as a compatibility fallback. For **ScalarWebAPI-compatible speakers**, configure `scalar_webapi_device` so Rusty Jack can wake the device when its Mac output is selected or when the daemon sees idle-to-active activity. See [Volume on external displays](#volume-on-external-displays) and [docs/USAGE.md](./docs/USAGE.md#scalar_webapi_device).

Full command reference: [docs/USAGE.md](./docs/USAGE.md). Troubleshooting: [docs/TROUBLESHOOTING.md](./docs/TROUBLESHOOTING.md).

---

## The problem

When macOS plays through an external display (DisplayPort / HDMI / many docks), **F10 / F11 / F12 often don’t control audible volume**. The monitor is usually a **fixed-gain digital output**: macOS can route audio to it, but there is nothing meaningful for the system volume slider or keyboard keys to adjust.

Built-in speakers and most Bluetooth headsets expose **software-controllable volume** in CoreAudio. External displays typically **do not**.

## Current Capabilities

| Capability | Command / config |
|------------|------------------|
| List output devices with transport, device name, active route, and routability | `list`, `list --hdmi` |
| Show current route, policy match, system virtual default, and volume | `status` |
| Switch once to preferred device or fallback from config | `apply` |
| Pick interactively or by device index | `picker`, `picker --index N` |
| Apply configured volume after a real switch | `volume` |
| Prefer the Rusty Jack native driver for connected HDMI/DisplayPort volume control; use eqMac only when already installed | automatic during `apply` / `picker` / `daemon` |
| Run a background auto-switch supervisor | `daemon` |
| Pause, resume, or uninstall the per-user LaunchAgent | `pause`, `resume`, `disable` |
| Wake ScalarWebAPI-compatible devices on output selection or idle-to-active daemon triggers | `scalar_webapi_device` |

Switching the default output to a **physical HDMI device alone does not fix volume keys**. Rusty Jack solves the routing and daemon automation side and now detects when connected HDMI/DisplayPort outputs need volume control. Its native HAL driver is the preferred path; **eqMac** is used only when it is already installed.

---

## Volume on external displays

### Why HDMI volume is hard

Physical HDMI/DP endpoints often have **no settable CoreAudio volume scalar**. macOS sends a fixed digital stream; keyboard volume has nothing to drive.

### HDMI/DisplayPort volume control

HDMI/DisplayPort volume control needs:

1. A **virtual HAL device** as the system default (what volume keys target).
2. An **app** that captures audio, applies software gain, and renders to the physical monitor.

Rusty Jack offers this path only when a connected HDMI/DisplayPort output is present. It prefers its own native HAL driver. If that driver is not installed and eqMac is already installed, it starts eqMac when you switch to an HDMI/DisplayPort device:

| HDMI/DP volume-control state | Behavior |
|------------------------------|----------|
| Rusty Jack native driver installed | Use it as the preferred HDMI/DP volume-control path |
| eqMac installed + running | No action |
| eqMac installed, not running | `open -a eqMac`, brief startup wait |
| No Rusty Jack driver and no eqMac | Recommend the Rusty Jack native driver |

Detection: Rusty Jack scans connected CoreAudio outputs for HDMI/DisplayPort transports before offering the driver path. It scans installed HAL `.driver` bundles for `com.the-hcma.rusty-jack.driver`, then checks for eqMac via `/Applications/eqMac.app` or `/Library/Audio/Plug-Ins/HAL/eqMac.driver` plus `pgrep -x eqMac`. If eqMac is not installed, Rusty Jack recommends its own driver rather than suggesting an eqMac install.

### Config `volume`

When set (0–100), rusty-jack uses it for the configured preferred output. Other outputs keep their own remembered volume in `~/.config/rusty-jack/device-volumes.json`; Rusty Jack records a non-preferred output's volume before switching away and restores it when switching back.

### Native driver

Run `rusty-jack install` with an HDMI/DisplayPort output connected. In an interactive terminal Rusty Jack offers to install `RustyJack.driver` to:

```text
~/Library/Audio/Plug-Ins/HAL/RustyJack.driver
```

The installer looks for a bundled driver next to the binary, under `../share/rusty-jack/RustyJack.driver` for Homebrew-style installs, or at `RUSTY_JACK_DRIVER_BUNDLE` for source/testing builds. `make install` builds the source bundle into `~/.cargo/share/rusty-jack/RustyJack.driver`; Homebrew installs the same bundle into `share/rusty-jack`. `rusty-jack uninstall` offers to remove the driver when it is installed. `rusty-jack upgrade` compares the bundled and installed driver and only offers a driver upgrade when the bundle materially changed.

The packaged driver currently contains the loadable HAL skeleton. It is safe for macOS to load, but the virtual output and passthrough software-volume pipeline are the next driver milestone.

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
| `picker` | Interactive menu or `--index N` to switch; pauses a running daemon after confirmation when you pick a non-preferred output |
| `resume` | Re-enable launchd agent; synchronously routes and restores configured `volume` first |
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
| `preferred_device.name` | Human-readable CoreAudio device name from `list` |
| `preferred_device.uid` | Stable CoreAudio UID from `list`; this is the selector Rusty Jack uses |
| `preferred_device_uid` | Legacy; use `preferred_device.uid` |
| `fallback_uids` | Try in order if preferred is unplugged; empty means use the built-in output automatically when available |
| `also_set_system_output` | Also set system/alert output (default `true`) |
| `volume` | 0–100; restore on route switches and daemon startup/resume |
| `auto_switch` | Master enable for the daemon loop |
| `poll_interval_ms` | Daemon route check interval (default `3000`) |
| `switch_delay_ms` | Delay after daemon switch before wake hooks (default `500`) |
| `activity_idle_threshold_ms` | Idle time that counts as away before an idle-to-active trigger (default `60000`) |
| `activity_poll_interval_ms` | Daemon idle-state sampling interval (default `1000`) |
| `scalar_webapi_device` | Wake ScalarWebAPI device on `apply`, `picker`, daemon output switches, and daemon idle-to-active triggers using discovered ScalarWebAPI endpoint |

Minimal example:

```json
{
  "version": 1,
  "preferred_device": {
    "name": "HDMI",
    "uid": "PASTE-UID-FROM-rusty-jack-list"
  },
  "fallback_uids": [],
  "also_set_system_output": true,
  "volume": 13
}
```

`match`, `exclude`, and `logging` in `config.example.json` are reserved for future behavior and currently ignored by the loader.

ScalarWebAPI device example: [`config.example.scalar-webapi-device.json`](./config.example.scalar-webapi-device.json). Other devices should work if they expose the same ScalarWebAPI service; Rusty Jack discovers the advertised endpoint and uses `system.getPowerStatus` / `system.setPowerStatus`.

Expected compatible Sony devices include Sony `SRS-ZR5` (the model this integration has been tested with), `SRS-ZR7`, `HT-NT5`, `HT-ST5000`, and `STR-DN1080`. This list is not exhaustive; compatibility depends on the device advertising a ScalarWebAPI endpoint on the local network.

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

`install` creates `~/.config/rusty-jack/config.json` when needed, prompting for a preferred output and an optional explicit fallback output. If no explicit fallback is configured, Rusty Jack still uses the Mac's built-in output automatically when available. It then renders `~/Library/LaunchAgents/com.example.rusty-jack.plist` from the bundled template, points it at the current `rusty-jack` binary, creates `~/Library/Logs`, and bootstraps the job in your `gui/<uid>` launchd domain.

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
git clone https://github.com/the-hcma/rusty-jack.git
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

- Volume keys dead on HDMI/DisplayPort without the Rusty Jack driver or installed eqMac fallback
- eqMac installed but volume still wrong
- Zoom / virtual devices in the picker
- Policy “no change” / wrong monitor

---

## Roadmap

| Area | Status |
|------|--------|
| Routing CLI, HDMI/DisplayPort volume-control detection, eqMac fallback, volume retries, daemon polling | Implemented |
| ScalarWebAPI device wake via ScalarWebAPI + daemon idle polling | Implemented |
| LaunchAgent install, upgrade, uninstall, and status helper | Implemented |
| Native event listener refinements for activity detection | Planned |
| Native driver bundle + installer | Implemented: loadable HAL skeleton packaged for source/Homebrew installs |

Full plan: **[IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md)**.

---

## Packaging

Rusty Jack is distributed through the Homebrew tap `the-hcma/tap`:

```bash
brew tap the-hcma/tap
brew install rusty-jack
```

The formula source lives at [`packaging/homebrew/rusty-jack.rb`](./packaging/homebrew/rusty-jack.rb).
Release steps are in [docs/RELEASING.md](./docs/RELEASING.md).

---

## License

Copyright (c) 2026 Henrique Andrade / thehcma.

MIT
