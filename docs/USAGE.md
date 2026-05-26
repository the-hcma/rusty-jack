# Rusty Jack — usage reference

Command-line reference for the current release. Rusty Jack currently ships routing, picker, status, eqMac integration for HDMI/DisplayPort keyboard volume-key control, daemon polling, LaunchAgent install/pause/resume/uninstall/upgrade controls, and ScalarWebAPI-compatible device wake support. Native HDMI/DP volume without eqMac remains future driver work.

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

**Volume:** For this one-shot command, if `volume` is set in config, it is applied only when a switch occurs (not on `no_change`).

**eqMac:** Started automatically when the target is HDMI-class and eqMac is installed but not running.

---

## `daemon`

Long-running background supervisor used by launchd.

```bash
rusty-jack daemon
rusty-jack --config ~/.config/rusty-jack/config.json daemon
```

The daemon reloads config before each scheduled poll, resolves the preferred/fallback output, and switches only when the active routed output differs. This includes eqMac-routed HDMI/DisplayPort, where the raw CoreAudio default may be the virtual eqMac device while the audible route is already correct. On startup, including a fresh login or after `upgrade`, it selects or preserves the preferred ScalarWebAPI-backed output and sends a wake command when that output is selected. During the initial startup grace window, retry ticks keep trying ScalarWebAPI wake without falling back so the network and device discovery have time to settle; the grace window is at least 30 seconds and grows with `scalar_webapi.wake_debounce_ms` if configured longer. Later scheduled polls check ScalarWebAPI reachability, but they only switch to fallback after the Mac's network access fingerprint changes: active default interface, default gateway, or interface IP address. If that fingerprint is stable, a ScalarWebAPI timeout is treated as transient and the daemon keeps the ScalarWebAPI-backed Mac output selected. When the Mac has been idle longer than `activity_idle_threshold_ms` and then becomes active again, the daemon runs an extra activity-triggered tick; if the configured ScalarWebAPI output is already selected, it sends a wake command subject to `scalar_webapi.wake_debounce_ms`.

| Field | Default | Meaning |
|-------|---------|---------|
| `auto_switch` | `true` | Master enable for daemon switching and activity wake behavior |
| `poll_interval_ms` | `3000` | Scheduled route check interval |
| `switch_delay_ms` | `500` | Delay after a daemon switch before wake hooks |
| `activity_idle_threshold_ms` | `60000` | Idle duration that counts as away |
| `activity_poll_interval_ms` | `1000` | How often the daemon samples macOS idle time |

---

## `disable` / `install` / `pause` / `resume` / `uninstall` / `upgrade`

Control the per-user LaunchAgent `com.example.rusty-jack` (template in `launchd/`).

```bash
rusty-jack disable [--json]
rusty-jack install [--json]
rusty-jack pause [--json]
rusty-jack resume [--json]
rusty-jack uninstall [--json]
rusty-jack upgrade [--json]
```

| Command | Plist | Agent |
|---------|-------|-------|
| `disable` | **Removed** | Stopped + disabled |
| `install` | Written for current binary | Enabled + started |
| `pause` | Kept | Stopped + disabled |
| `resume` | Kept | Enabled + started |
| `uninstall` | **Removed** | Stopped + disabled |
| `upgrade` | Rewritten for current binary | Paused, then resumed if it was running |

LaunchAgents run in a single user’s GUI launchd domain (`gui/<uid>`), not system-wide. Each macOS account that wants auto-routing can install its own `~/Library/LaunchAgents/com.example.rusty-jack.plist`; the jobs do not conflict across users.

### Install

Install the binary through Homebrew:

```bash
brew tap the-hcma/tap
brew install rusty-jack
```

Then let Rusty Jack create config, prompt for preferred and optional fallback outputs, and load the LaunchAgent:

```bash
rusty-jack install
```

For a source checkout, use `make install` first:

```bash
make install
rusty-jack install
```

`install` creates `~/.config/rusty-jack/config.json` when it is missing. In an interactive terminal it prompts for the preferred output and an optional explicit fallback output. If no explicit fallback is configured, the daemon still uses the Mac's built-in output automatically when available. In `--json` mode it avoids prompts and uses deterministic defaults from the live device list. It then writes `~/Library/LaunchAgents/com.example.rusty-jack.plist`, creates `~/Library/Logs`, bootstraps the job in the current user’s launchd domain, and starts `rusty-jack daemon`. Logs go to `~/Library/Logs/rusty-jack.stdout.log` and `~/Library/Logs/rusty-jack.stderr.log`.

### Pause, Resume, Uninstall

```bash
rusty-jack pause      # stop auto-routing; keep plist installed
rusty-jack resume     # re-enable and start the plist
rusty-jack uninstall  # stop, disable, remove plist, offer config cleanup
rusty-jack uninstall --remove-config  # also remove default config without prompting
```

`resume` applies the configured route and volume synchronously, then starts the daemon. `disable` remains available for daemon-only removal and always keeps `~/.config/rusty-jack/config.json`. `uninstall` prompts before removing the default config in interactive mode; `--keep-config` keeps it without prompting. Neither command deletes log files.

### Update

Replace the binary first, then refresh the LaunchAgent:

```bash
git pull
make upgrade
```

`make upgrade` installs the new binary once, then runs `rusty-jack upgrade`. The CLI `upgrade` command itself does not download source or build a new binary. It rewrites the plist to point at the current `rusty-jack` executable, reports the before/after version and commit, and automatically pauses/resumes the daemon if it was running. If the daemon was paused before the upgrade, it stays paused; if the daemon was not installed yet, `upgrade` installs it.

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
- ScalarWebAPI power-state notes refresh while the interactive picker is open.

When the daemon is running and you pick a device other than the configured preferred output, `picker` asks for confirmation, pauses auto-routing, and records the override reason. The confirmation defaults to yes and shows `Continue` on its own colored prompt line. You must run `rusty-jack resume` to re-enable the daemon. Non-interactive picker calls cannot confirm this pause; pause the daemon first or rerun picker interactively.

**Interactive legend:**

```
Select output device (↑↓, Enter, p preferred, Esc)  (> active, * preferred, dim = not routable)
```

| Visual | Meaning |
|--------|---------|
| `>` green | Active route |
| `*` cyan | Config preferred |
| dim | Not selectable (e.g. ZoomAudioDevice) |
| `>*` | Active and preferred |

Press **p** to switch directly to the configured preferred device. Press **Esc** to cancel without switching.

---

## `status`

Snapshot of devices, virtual system default (eqMac footer when applicable), policy evaluation, and per-user LaunchAgent state.

```bash
rusty-jack status
rusty-jack status --json
rusty-jack status --config ~/.config/rusty-jack/config.json
```

Policy block fields (aligned columns):

- `configured`, `config`, `monitor`, `preferred`, `active`, `matches`, `auto_switch`
- `config volume`, `volume` (current effective %)
- `note` (human-readable policy message)

Daemon block fields include `installed`, `running`, and `paused` booleans, plus the launchd label, service, plist path, and PID when available. State values:

- `running` — LaunchAgent plist exists and launchd reports the job loaded; PID is shown when available.
- `paused` — plist exists but launchd does not currently have the job loaded. If picker paused the daemon for a manual output override, `status` includes a `reason` and a note telling you to run `rusty-jack resume`.
- `not_installed` — plist is not present under `~/Library/LaunchAgents`.

Config is optional for `status`; without it, policy reports “not configured”.

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

Array of UIDs tried in order when preferred is missing or not alive. Leave it empty to use the Mac's internal built-in speaker output automatically when it is connected.

### Daemon fields

| Field | Default | Description |
|-------|---------|-------------|
| `auto_switch` | `true` | Enables daemon policy enforcement. When false, `daemon` keeps running but does not switch or activity-wake. |
| `poll_interval_ms` | `3000` | Scheduled daemon policy check interval. Must be greater than zero. |
| `switch_delay_ms` | `500` | Delay after daemon-initiated route switches before wake hooks run. |
| `activity_idle_threshold_ms` | `60000` | Idle duration that must be reached before the next active sample counts as an idle-to-active transition. Must be greater than zero. |
| `activity_poll_interval_ms` | `1000` | How often `daemon` samples macOS idle time between scheduled route checks. Must be greater than zero. |

### `volume`

Integer 0–100. Created automatically from the preferred route's current effective volume when `install` can read it. This config value is authoritative for the configured preferred output. Other outputs use per-device remembered volume stored in `~/.config/rusty-jack/device-volumes.json`; Rusty Jack records a non-preferred output's volume before switching away and restores it when switching back. Scheduled no-op polls do not keep forcing volume, so manual volume changes are not fought every poll. Uses retry + readback for eqMac compatibility.

### `scalar_webapi`

Optional block for waking a ScalarWebAPI-compatible device. When enabled and `triggers` includes `output_selected`, `apply`, `picker`, and daemon-initiated output switches discover the device's advertised ScalarWebAPI endpoint, then send `system.setPowerStatus` when the selected Mac output matches `scalar_webapi.mac_output`. When `triggers` includes `keyboard` or `mouse`, `daemon` also wakes the device on idle-to-active transitions if that Mac output is already selected. `port` defaults to `10000` and is only used as a fallback if discovery is unavailable. See `config.example.scalarwebapi.json`.

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `false` | Enables ScalarWebAPI wake integration. |
| `model` | `ScalarWebAPI device` | Human-readable model hint for docs/logging. |
| `host` | none | Hostname, FQDN, or IP used for discovery fallback and configured endpoint construction. Required when enabled. |
| `port` | `10000` | Fallback ScalarWebAPI port when SSDP discovery is unavailable. |
| `path` | protocol default | ScalarWebAPI base path. Usually omit this unless discovery is unavailable and your device needs an override. |
| `mac_output` | none | Device selector for the Mac output connected to the device. Required when enabled. |
| `triggers` | `["keyboard", "mouse", "output_selected"]` | Wake on explicit output selection and/or daemon idle-to-active activity. |
| `wake_debounce_ms` | `30000` | Minimum time between activity-triggered wake attempts. |
| `request_timeout_ms` | `3000` | Network timeout for speaker requests. |
| `require_quick_start` | `true` | Documents the expectation that the device has its network standby/wake option enabled. |

Other devices should work if they expose the same ScalarWebAPI service.

### Reserved example keys

`match`, `exclude`, and `logging` appear in `config.example.json` as roadmap placeholders. The current loader ignores unknown keys and does not apply those settings.

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
make upgrade    # install once, then restart LaunchAgent
make uninstall  # remove LaunchAgent and cargo-installed binary
```

Cross-compilation targets used in CI: `aarch64-apple-darwin`, `x86_64-apple-darwin`.

`MACOSX_DEPLOYMENT_TARGET=12.0` is set in the Makefile.

---

Copyright (c) 2026 Henrique Andrade / thehcma.
