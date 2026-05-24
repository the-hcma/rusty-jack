# Rusty Jack

macOS CLI daemon that makes **hardware volume keys work** when system audio goes to an **HDMI, DisplayPort, or USB-C dock** monitor — the same core problem [eqMac](https://github.com/bitgapp/eqMac) solves, without the menu bar app or EQ.

> *Your HDMI output, on deck — and the volume keys actually work.*

## The problem

When macOS plays through an external display (DisplayPort / HDMI / many docks), **F10 / F11 / F12 often don’t control audible volume**. The monitor is usually a **fixed-gain digital output**: macOS can route audio to it, but there is nothing meaningful for the system volume slider or keyboard keys to adjust.

Built-in speakers and most Bluetooth headsets expose **software-controllable volume** in CoreAudio. External displays typically **do not**.

## What eqMac does (and what we’re building)

eqMac installs a **virtual audio device** as the system default. Volume keys adjust software gain on that virtual path; eqMac **re-renders** the audio to your real HDMI/DP output at the level you chose.

**Rusty Jack** aims for the same outcome — **keyboard volume control on HDMI/DP** — as a **headless, launchd-friendly CLI** with JSON config. No EQ, no per-app mixer, no menu bar.

| Piece | Role |
|-------|------|
| **Virtual driver (planned)** | System default output that volume keys can control |
| **Passthrough + software volume (planned)** | Apply gain, send, to chosen physical monitor/dock |
| **Routing + daemon (in progress)** | Pick and keep the right HDMI/DP device connected |

Switching the default output to a physical HDMI device alone **does not** fix volume keys. That requires the virtual-device pipeline (see [IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md) §0 and Phase 7).

### Sony SRS-ZR5 wake (planned — Phase 8)

For a Mac **line-out** cabled to a **Sony SRS-ZR5** (Sony **ScalarWebAPI** / Songpal protocol), the speaker is often in standby. **Planned:** when line-out is the preferred/active output and **mouse or keyboard activity** is detected, rusty-jack will POST to the speaker’s local REST API (`system.setPowerStatus`) using a **native Rust client** — no Python or [python-songpal](https://github.com/rytilahti/python-songpal) at runtime (that project is protocol reference only). Configure `sony_speaker` in `config.example.json` — see [IMPLEMENTATION_PLAN.md §1.1](./IMPLEMENTATION_PLAN.md).

## Platform

- **macOS 12 Monterey** or later (Intel and Apple Silicon)
- **macOS only** — not built for or tested on Linux
- Release builds cross-compile **`aarch64-apple-darwin`** and **`x86_64-apple-darwin`** (see `scripts/build-universal.sh`)
- **GitHub Actions CI** runs on **`macos-14`** runners (see [`.github/workflows/ci.yml`](.github/workflows/ci.yml))

## Status

**Phase 1:** device enumeration, `list`, and `status` on macOS (transport, monitor name, active device highlighting, policy match). Routing write path, daemon, and the **virtual driver + software volume** path are planned — see [IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md).

### Build

Requires [Rust](https://rustup.rs/) 1.85+ and macOS 12+ for CoreAudio.

One-time setup (if `cargo` is not found):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"    # add cargo to PATH in this shell
```

```bash
# From repo root
make build          # debug binary
make release        # optimized binary → target/release/rusty-jack
make test           # unit tests
make universal      # aarch64 + x86_64 + lipo (release)
make install        # cargo install --path . to ~/.cargo/bin
```

Or directly:

```bash
cargo build --release
./target/release/rusty-jack list
./target/release/rusty-jack list --hdmi
./target/release/rusty-jack list --json
./target/release/rusty-jack status
./target/release/rusty-jack status --json
```

### `list` command

| Command | Description |
|---------|-------------|
| `rusty-jack list` | All output devices (table with index, transport, UID) |
| `rusty-jack list --hdmi` | HDMI, DisplayPort, Thunderbolt, USB dock outputs only |
| `rusty-jack list --json` | JSON device list (works with `--hdmi`) |

### `status` command

| Command | Description |
|---------|-------------|
| `rusty-jack status` | Routing snapshot: device table (active row highlighted), virtual default details, policy match |
| `rusty-jack status --json` | Same fields as JSON |
| `rusty-jack status --config /path/to/config.json` | Evaluate policy against a specific config file |

Config is read from `--config`, `RUSTY_JACK_CONFIG`, or `~/.config/rusty-jack/config.json`.

### `apply` command

| Command | Description |
|---------|-------------|
| `rusty-jack apply` | Set default output to preferred device (or fallback) from config |
| `rusty-jack apply --json` | Same result as JSON (`switched` or `no_change`) |

Requires a valid config file. Resolves `preferred_device.monitor_name` or `uid`, then sets the system default output (and system/alert output when `also_set_system_output` is true).

### Configuration

Preferred output — by **monitor name** (when unique) or CoreAudio UID:

```json
"preferred_device": {
  "monitor_name": "DELL U3219Q"
}
```

Sony speaker wake (optional — omit on Macs without a networked ZR5). See [`config.example.sony.json`](./config.example.sony.json):

```json
"sony_speaker": {
  "enabled": true,
  "host": "sony.house.hcma",
  "port": 10000,
  "path": "sony",
  "mac_output": { "monitor_name": "Built-in Output" }
}
```

`host` accepts a hostname, FQDN, or IP address.

## Install via Homebrew (planned)

Yes — a Rust macOS CLI is a natural fit for Homebrew. You ship a **native Mach-O binary**; users run:

```bash
brew install rusty-jack   # from your tap, or homebrew-core if accepted
rusty-jack agent install
```

### How distribution usually works

| Stage | What you do |
|-------|-------------|
| **1. Your tap** | Publish `homebrew-tap` with a formula that builds from source or installs release bottles |
| **2. Releases** | GitHub Actions builds `rusty-jack` for `aarch64-apple-darwin` and `x86_64-apple-darwin`, uploads tarballs |
| **3. Formula** | `brew install` downloads the bottle or runs `cargo install --locked --root $(brew --prefix)` |
| **4. Optional core** | Submit to [homebrew-core](https://docs.brew.sh/Adding-Software-to-Homebrew) once stable (macOS-only formulae are OK with `depends_on :macos`) |

Example formula sketch lives in [`packaging/homebrew/rusty-jack.rb`](./packaging/homebrew/rusty-jack.rb).

### Homebrew vs direct download

- **Homebrew** — Users get updates with `brew upgrade`, binary on `PATH`, no Rust toolchain required.
- **Notarization** — Recommended for *drag-and-drop* `.app` or standalone `.pkg`; important once the **virtual audio driver** ships (Phase 7).
- **launchd** — Still per-user: `rusty-jack agent install` writes `~/Library/LaunchAgents/...` (no root for the agent itself; driver install will require admin).

## Config (planned)

`~/.config/rusty-jack/config.json` — preferred physical output UID, auto-switch policy, volume behavior — see `config.example.json`.

## Uninstall (planned)

```bash
rusty-jack uninstall              # stop agent, remove plist, restore prior output (if saved)
rusty-jack uninstall --purge -y   # also remove config, state, logs, virtual driver
brew uninstall rusty-jack         # runs the same cleanup via formula hook
```

## License

MIT (update as you prefer)
