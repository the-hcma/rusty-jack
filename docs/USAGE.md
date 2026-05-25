# Rusty Jack — usage reference

Command-line reference for the current release. For architecture and roadmap see [IMPLEMENTATION_PLAN.md](../IMPLEMENTATION_PLAN.md).

## Global options

```
rusty-jack [OPTIONS] <COMMAND>
```

| Option | Description |
|--------|-------------|
| `--config PATH` | Config JSON (overrides `RUSTY_JACK_CONFIG` and `~/.config/rusty-jack/config.json`) |
| `--help` | Subcommands (alphabetical) + version with **git commit** |
| `--version` | Short version string |

Environment:

| Variable | Description |
|----------|-------------|
| `RUSTY_JACK_CONFIG` | Path to config file |
| `HDMI_SOUND_CONTROLLER_CONFIG` | Legacy alias for `RUSTY_JACK_CONFIG` |
| `NO_COLOR` | Disable ANSI colors in tables and picker |

---

## `list`

Enumerate CoreAudio output devices.

```bash
rusty-jack list
rusty-jack list --hdmi
rusty-jack list --json
```

| Flag | Description |
|------|-------------|
| `--hdmi` | Only HDMI, DisplayPort, Thunderbolt, USB |
| `--json` | `DeviceList` JSON |

Table columns: **IDX**, **ACT** (`>` = active route), **ALIVE**, **TRANSPORT**, **DEVICE**, **MONITOR**, **UID**.

Non-selectable devices (aggregates, some virtual apps) may appear dimmed when color is enabled.

---

## `status`

Snapshot of devices, virtual system default (eqMac footer when applicable), and policy evaluation.

```bash
rusty-jack status
rusty-jack status --json
rusty-jack status --config ~/.config/rusty-jack/config.json
```

Policy block fields (aligned columns):

- `configured`, `config`, `monitor`, `preferred`, `active`, `matches`, `auto_switch`
- `config volume`, `volume` (current effective %)
- `note` (human-readable policy message)

Config is optional for `status`; without it, policy reports “not configured”.

---

## `apply`

One-shot apply of config policy: resolve preferred device (or fallback), ensure eqMac if HDMI-class, switch default output.

```bash
rusty-jack apply
rusty-jack apply --json
```

Requires valid config. Results:

| JSON `action` | Meaning |
|---------------|---------|
| `switched` | Default output changed; may include `volume` ensure result |
| `no_change` | Already on target |

**Volume:** If `volume` is set in config, applied only when a switch occurs (not on `no_change`).

**eqMac:** Started automatically when the target is HDMI-class and eqMac is installed but not running.

---

## `picker`

Pick an output device and switch to it.

```bash
rusty-jack picker                    # interactive (TTY required)
rusty-jack picker --index 0          # same IDX as `list`
rusty-jack picker --json
```

| Flag | Description |
|------|-------------|
| `--index N` | Non-interactive; fails if device is not routable |
| `--json` | Apply result or `{"status":"cancelled"}` |

Config is optional. When present:

- `also_set_system_output` from config (default `true` if no config)
- `volume` applied only when you pick the **configured preferred** device and a switch happens

**Interactive legend:**

```
Select output device (↑↓, Enter, Esc)  (> active, * preferred, dim = not routable)
```

| Visual | Meaning |
|--------|---------|
| `>` green | Active route |
| `*` cyan | Config preferred |
| dim | Not selectable (e.g. ZoomAudioDevice) |
| `>*` | Active and preferred |

Press **Esc** to cancel without switching.

---

## `pause` / `resume` / `disable`

Control the LaunchAgent `com.example.rusty-jack` (template in `launchd/`).

```bash
rusty-jack pause [--json]
rusty-jack resume [--json]
rusty-jack disable [--json]
```

| Command | Plist | Agent |
|---------|-------|-------|
| `pause` | Kept | Stopped + disabled |
| `resume` | Kept | Enabled + started |
| `disable` | **Removed** | Stopped + disabled |

---

## `daemon`

Reserved for the background supervisor. **Not implemented** — running it will error. Use `apply` manually until the daemon lands.

---

## Configuration file

Path resolution order:

1. `--config` on CLI  
2. `RUSTY_JACK_CONFIG`  
3. `~/.config/rusty-jack/config.json`

### Minimal example

```json
{
  "version": 1,
  "preferred_device": {
    "monitor_name": "DELL U3219Q"
  },
  "also_set_system_output": true,
  "volume": 13
}
```

### `preferred_device`

Use **either** (or both; `preferred_device` wins when set):

- `monitor_name` — product name from `list` MONITOR column (must be unique among connected devices)
- `uid` — stable CoreAudio UID from `list`

### `fallback_uids`

Array of UIDs tried in order when preferred is missing or not alive.

### `volume`

Integer 0–100. Applied on switch to preferred device only (`apply` / `picker` when preferred matches). Uses retry + readback for eqMac compatibility.

### `sony_speaker`

Optional block for waking a Sony SRS-ZR5 or similar Songpal / ScalarWebAPI speaker. When enabled and `triggers` includes `output_selected`, `apply` and `picker` discover the speaker's advertised ScalarWebAPI endpoint, then send `system.setPowerStatus` when the selected Mac output matches `sony_speaker.mac_output`. `port` defaults to `10000` and is only used as a fallback if discovery is unavailable. Keyboard/mouse activity triggers are still daemon work. See `config.example.sony.json`.

Other Sony speakers may also work if they expose the same ScalarWebAPI. If you verify another model, please consider contributing a device info file to [python-songpal on GitHub](https://github.com/rytilahti/python-songpal); the [`python-songpal` PyPI package](https://pypi.org/project/python-songpal/) provides the `songpal dump-devinfo` helper.

---

## Build

From repository root:

```bash
make build      # debug
make release    # optimized
make test
make fmt        # check formatting (CI)
make clippy     # lint (CI)
make universal  # fat binary
make install    # cargo install --path .
```

Cross-compilation targets used in CI: `aarch64-apple-darwin`, `x86_64-apple-darwin`.

`MACOSX_DEPLOYMENT_TARGET=12.0` is set in the Makefile.
