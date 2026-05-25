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

### Build (local — debug & release)

Requires **macOS 12 Monterey or later**, **Apple Silicon or Intel**, and **[Rust](https://rustup.rs/) 1.85+** (see `rust-version` in `Cargo.toml`). CoreAudio is macOS-only; build on a Mac, not Linux.

#### 1. One-time setup on a new Mac

```bash
# Xcode command-line tools (compiler + SDK) — skip if already installed
xcode-select --install

# Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

rustc --version    # should be ≥ 1.85
```

#### 2. Get the source

```bash
git clone https://github.com/thehcma/rusty-jack.git
cd rusty-jack
```

#### 3. Debug vs release

| Build | Command | Binary path | When to use |
|-------|---------|-------------|-------------|
| **Debug** | `make build` or `cargo build` | `target/debug/rusty-jack` | Fast compile while developing; larger binary, no LTO |
| **Release** | `make release` or `cargo build --release` | `target/release/rusty-jack` | What you should run day-to-day; optimized, stripped |

Debug builds compile faster and include debug symbols (useful with `lldb`). Release builds are smaller, faster at runtime, and match what CI produces.

```bash
# Debug — quick iteration
make build
./target/debug/rusty-jack --help
./target/debug/rusty-jack list

# Release — verify like a “real” install
make release
./target/release/rusty-jack --help
./target/release/rusty-jack status
```

`--help` prints the version **and embedded git commit** (from `build.rs`), e.g. `rusty-jack 0.1.0 (commit abc1234)` — useful to confirm which revision is on the machine.

#### 4. Run without installing to PATH

```bash
# Either binary works; substitute debug/release as needed
./target/release/rusty-jack list
./target/release/rusty-jack list --hdmi
./target/release/rusty-jack status
./target/release/rusty-jack picker          # interactive; needs a TTY
./target/release/rusty-jack picker --index 0
./target/release/rusty-jack apply           # needs ~/.config/rusty-jack/config.json
```

Copy `config.example.json` to `~/.config/rusty-jack/config.json` and edit `preferred_device` before testing `apply` / policy in `status`.

#### 5. Optional: install to `~/.cargo/bin`

```bash
make install          # builds release, then cargo install --path .
rusty-jack --help     # on PATH if ~/.cargo/bin is in your shell profile
```

#### 6. Universal binary (Apple Silicon + Intel in one file)

For distributing a single Mach-O to mixed Macs:

```bash
make universal        # runs scripts/build-universal.sh → target/release/rusty-jack-universal
```

#### 7. Verify on another Mac (checklist)

```bash
make test             # unit + integration tests (needs macOS)
make clippy           # optional lint pass

# Smoke test after build
./target/release/rusty-jack list
./target/release/rusty-jack status
./target/release/rusty-jack picker --index 0 --json   # non-interactive switch test
```

If you use eqMac or Zoom virtual devices: `list` / `status` show them; **ZoomAudioDevice** appears dimmed in `picker` and cannot be selected (app virtual driver, not a speaker).

#### Makefile targets

```bash
make build          # debug → target/debug/rusty-jack
make release        # release → target/release/rusty-jack
make test           # cargo test --all-targets
make fmt            # cargo fmt
make clippy         # cargo clippy --all-targets
make universal      # fat binary (aarch64 + x86_64)
make install        # release + cargo install --path .
make clean          # cargo clean
```

Or use `cargo` directly (same results):

```bash
cargo build                    # debug
cargo build --release          # release
cargo test
cargo run -- list              # debug via cargo run
cargo run --release -- status  # release via cargo run
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

Requires a valid config file. Resolves `preferred_device.monitor_name` or `uid`, then sets the system default output (and system/alert output when `also_set_system_output` is true). When `volume` is set in config, applies it only on an actual switch.

### `picker` command

| Command | Description |
|---------|-------------|
| `rusty-jack picker` | Interactive menu to choose an output device and switch to it |
| `rusty-jack picker --index N` | Switch to device index `N` (same IDX as `list`) without a menu |
| `rusty-jack picker --json` | Same result as JSON (`switched` or `no_change`) |

Does not require config. When config is present, uses `also_set_system_output` from it; otherwise defaults to `true`. If you pick the **configured preferred device** and a switch occurs, config `volume` is applied (same as `apply`). Other picks leave volume unchanged. Press **Esc** to cancel without switching.

### Daemon control

| Command | Description |
|---------|-------------|
| `rusty-jack pause` | Stop auto-routing; keeps the LaunchAgent plist installed |
| `rusty-jack resume` | Re-enable and start a paused daemon |
| `rusty-jack disable` | Uninstall: stop, disable, and **remove** the LaunchAgent plist |

Add `--json` to any of these for machine-readable output.

- **pause** — `launchctl bootout` + `disable`; plist stays at `~/Library/LaunchAgents/com.example.rusty-jack.plist`. Use when you want to temporarily stop auto-routing.
- **resume** — `launchctl enable` + `bootstrap` to start the daemon again after pause.
- **disable** — full cleanup/uninstall from launchd (plist removed). Use when removing rusty-jack entirely.

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
