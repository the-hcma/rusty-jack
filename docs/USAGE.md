# Rusty Jack — usage reference

Command-line reference for the current release. Rusty Jack currently ships routing, picker, status, HDMI/DisplayPort volume-control detection, native driver lifecycle hooks, an explicit eqMac/Rusty Jack driver swap test workflow, eqMac compatibility fallback when eqMac is already installed, daemon polling (idle or optional event-tap activity sampling), LaunchAgent install/pause/resume/uninstall/upgrade controls (including config and log purge), ScalarWebAPI-compatible device wake (`scalar-webapi-device discover`), and Homebrew uninstall hooks that call `rusty-jack disable`.

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
| `RUSTY_JACK_DRIVER_BUNDLE` | Optional path to a `RustyJack.driver` bundle when testing or installing from a source checkout |
| `HDMI_SOUND_CONTROLLER_CONFIG` | Legacy alias for `RUSTY_JACK_CONFIG` |
| `NO_COLOR` | Disable ANSI colors in tables and picker |

---

## `config`

Helpers for creating and validating config files.

```bash
rusty-jack config init
rusty-jack config init --json

rusty-jack config validate
rusty-jack config validate --json
```

- `config init` creates the config file when it is missing. In an interactive terminal it uses the same first-run prompts as `install`: preferred output, optional fallback, optional ScalarWebAPI speaker wake (including LAN discovery when creating a new config). In non-interactive mode it picks defaults and skips ScalarWebAPI setup.
- `config validate` loads the config, validates it, and rewrites it in canonical JSON key order when needed.

---

## `apply`

One-shot apply of config policy: resolve preferred device (or fallback), ensure HDMI/DisplayPort volume control if the target is an HDMI/DP output, switch default output.

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

**HDMI/DisplayPort volume control:** Shipped `RustyJack.driver` bundles are **not Developer ID–signed**, so macOS usually will not load them for end users. **Use eqMac** when it is already installed. A locally signed driver is for development; see [DRIVER_SIGNING.md](./DRIVER_SIGNING.md).

---

## `daemon`

Long-running background supervisor used by launchd.

```bash
rusty-jack daemon
rusty-jack --config ~/.config/rusty-jack/config.json daemon
```

The daemon reloads config before each scheduled poll, resolves the preferred/fallback output, and switches only when the active routed output differs. This includes HDMI/DisplayPort routes through a virtual volume-control device, where the raw CoreAudio default may be virtual while the audible route is already correct. On startup, including a fresh login or after `upgrade`, it selects or preserves the preferred ScalarWebAPI-backed output and sends a wake command when that output is selected. For HDMI/DisplayPort routes using installed eqMac fallback, startup ticks restart eqMac before re-applying the route so macOS wake does not leave eqMac running but silent; while the Mac stays active, scheduled, keep-awake, and idle-to-active (unlock/activity) ticks check that eqMac still owns the CoreAudio virtual default—if the app process is alive but the virtual default is missing, they restart eqMac (60s cooldown) so volume keys work again after login or unlock without requiring a LaunchAgent restart. During the initial startup grace window, retry ticks keep trying ScalarWebAPI wake without falling back so the network and device discovery have time to settle; the grace window is at least 30 seconds and grows with `scalar_webapi_device.wake_debounce_ms` if configured longer. Later scheduled polls check ScalarWebAPI reachability, but they only switch to fallback after the Mac's network access fingerprint changes: active default interface, default gateway, or interface IP address. If that fingerprint is stable, a ScalarWebAPI timeout is treated as transient and the daemon keeps the ScalarWebAPI-backed Mac output selected. While the Mac is active (idle below `activity_idle_threshold_ms`), the daemon periodically re-checks ScalarWebAPI power status on the configured output and sends a wake command when the device is not active. After a successful `setPowerStatus`, it waits `scalar_webapi_device.wake_debounce_ms` before sending another wake command (failed sends are not debounced). When the Mac has been idle longer than `activity_idle_threshold_ms` and then becomes active again, the daemon runs an immediate activity-triggered wake attempt subject to the same post-success debounce. Activity sampling uses `activity_monitor`: `idle` (default, macOS idle time) or `event_tap` (listen-only CoreGraphics keyboard/mouse tap; falls back to `idle` when unavailable, disabled, or silent — see [Event tap activity monitor](#event-tap-activity-monitor) below).

| Field | Default | Meaning |
|-------|---------|---------|
| `auto_switch` | `true` | Master enable for daemon switching and activity wake behavior |
| `poll_interval_ms` | `3000` | Scheduled route check interval |
| `switch_delay_ms` | `500` | Delay after a daemon switch before wake hooks |
| `activity_idle_threshold_ms` | `60000` | Idle duration that counts as away |
| `activity_poll_interval_ms` | `1000` | How often the daemon samples macOS idle time |
| `activity_monitor` | `idle` | `idle` (macOS idle time) or `event_tap` (listen-only keyboard/mouse tap for activity detection; **does not log keystrokes**; falls back to `idle` when unavailable) |
| `activity_active_confirm_ms` | `5000` | Activity must stay below `activity_idle_threshold_ms` for this long before an idle→active transition counts. `0` disables confirmation. |
| `activity_event_tap_include_mouse_move` | `false` | When using `event_tap`, count `MouseMoved` events as activity. Leave `false` to reduce Bluetooth pointer jitter false wakes. |

### Event tap activity monitor

When `activity_monitor` is `event_tap`, the daemon installs a **listen-only** CoreGraphics event tap to detect that you resumed using the Mac (for ScalarWebAPI speaker wake gating).

#### Does not log or record keystrokes

Rusty Jack **does not log, record, store, or transmit what you type**. The event tap is not a keylogger.

macOS may ask for permission with wording like “receive keystrokes from any application.” That is Apple’s generic label for Accessibility / input monitoring. Rusty Jack uses the permission only to learn that **some** keyboard or pointer activity occurred — not **which keys** were pressed or **what text** was entered.

The tap never reads key codes, characters, passwords, clipboard content, or application names from events. Nothing you type is written to `~/Library/Logs/rusty-jack.log`, config, or disk.

#### What is recorded

| Recorded | Purpose | Example |
|----------|---------|---------|
| Time since last keyboard/pointer event | Idle vs active detection | `idle: 1.5s` in `rusty-jack status` |
| Coarse event-type label (in memory only) | Choose keyboard vs mouse wake trigger | `KeyDown`, `LeftMouseDown` |
| Idle/active snapshot fields | `status` and daemon activity logs | `[activity] idle→active transition` |

Event-type labels describe the *kind* of input (key press vs mouse click), not the content. They are not persisted as a history of your typing.

#### Permission and fallback

macOS requires **Accessibility** permission for the tap to receive events. **Restart the daemon after granting permission** so the tap is created with access enabled:

```bash
launchctl kickstart -k "gui/$(id -u)/com.example.rusty-jack"
```

If permission is missing at startup, macOS may disable the tap immediately or leave it silent. When `activity_event_tap_include_mouse_move` is `true`, Rusty Jack **falls back to the `idle` monitor** if the tap stops receiving events while the session is active. With the default `activity_event_tap_include_mouse_move: false`, a silent tap is **recreated** automatically (rate-limited) instead of falling back, so Bluetooth pointer jitter does not trigger speaker wakes. Look for `[activity] event tap using idle monitor fallback`, `[activity] event tap disabled by macOS`, `[activity] event tap appears silent`, or `event tap recreated after silent stall` in `~/Library/Logs/rusty-jack.log`. Interactive ScalarWebAPI install with `keyboard`/`mouse` wake triggers sets `event_tap` automatically.

`rusty-jack status` Activity block shows idle time, state, and the last idle→active transition. When the tap is working, `idle` stays low while you use the Mac; if `idle` climbs for hours while you are active, grant Accessibility permission and restart the daemon (or wait for an automatic tap recreate when `include_mouse_move` is `false`).

---

## `driver`

Explicit native-driver testing workflows. These commands do not run automatically during `install`; use them only when you want to temporarily move eqMac's system HAL driver aside and test Rusty Jack's user-scoped driver in its place.

```bash
rusty-jack driver swap-in [--json]
rusty-jack driver swap-out [--json]
```

`swap-in` backs up `/Library/Audio/Plug-Ins/HAL/eqMac.driver` to:

```text
~/.config/rusty-jack/driver-backups/eqMac.driver
```

It also writes backup metadata next to that bundle, then installs or refreshes `/Library/Audio/Plug-Ins/HAL/RustyJack.driver` from the packaged Rusty Jack bundle and restarts `coreaudiod`. Moving the system eqMac driver requires interactive confirmation and uses `sudo mv`; installing the Rusty Jack HAL driver for testing also uses `sudo`.

`swap-out` removes the user Rusty Jack driver and restores the managed eqMac backup to `/Library/Audio/Plug-Ins/HAL/eqMac.driver` with `sudo mv`. It is idempotent: if the Rusty Jack driver is already absent and eqMac is already restored, it reports up to date. If both the original eqMac driver and the managed backup exist, the command skips and asks you to inspect the state instead of overwriting anything.

In `--json` mode, Rusty Jack will not move or restore the system eqMac driver because that operation needs interactive confirmation. The JSON result includes a retry command such as `rusty-jack driver swap-in`. `rusty-jack status` shows the managed eqMac backup and the `rusty-jack driver swap-out` restore command when a backup exists.

---

## `disable` / `install` / `pause` / `resume` / `uninstall` / `upgrade`

Control the per-user LaunchAgent `com.example.rusty-jack` (template in `launchd/`).

```bash
rusty-jack disable [--json]
rusty-jack install [--json]
rusty-jack pause [--json]
rusty-jack resume [--json]
rusty-jack uninstall [--json] [--only-driver] [--no-restore-audio] [--purge] [--purge-logs] [--remove-config] [--keep-config]
rusty-jack upgrade [--json] [--force]
```

| Command | Plist | Agent |
|---------|-------|-------|
| `disable` | **Removed** | Stopped + disabled |
| `install` | Written for current binary | Enabled + started |
| `pause` | Kept | Stopped + disabled |
| `resume` | Kept | Enabled + started |
| `uninstall` | **Removed** | Stopped + disabled |
| `upgrade` | Rewritten only when needed | Paused, then resumed if it was running |

LaunchAgents run in a single user’s GUI launchd domain (`gui/<uid>`), not system-wide. Each macOS account that wants auto-routing can install its own `~/Library/LaunchAgents/com.example.rusty-jack.plist`; the jobs do not conflict across users. Activity-based ScalarWebAPI wake and eqMac routing apply to the installing user’s audio session only — if multiple people use the same Mac, run `rusty-jack install` in each account that should auto-route and wake devices.

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

`install` creates `~/.config/rusty-jack/config.json` when it is missing. If the config already exists, Rusty Jack shows the config path and current settings, updates readable `name` labels for known UIDs, and offers additive changes such as choosing a missing preferred output or adding an explicit fallback. It does not recreate the file or drop custom settings like `scalar_webapi_device`. Interactive reconfigure saves a timestamped backup under `config-backups/` next to the config file before applying changes. ScalarWebAPI reconfigure also scans the LAN, confirms whether the configured host appears in the scan, and offers other discovered speakers before falling back to manual host entry. Existing configs that enable ScalarWebAPI with a partial trigger list are offered a trigger upgrade during interactive `install`. In `--json` mode it avoids prompts and applies only non-interactive migrations. If a stale eqMac HAL driver is present without the eqMac app, interactive `install` offers to remove it with `sudo rm -rf /Library/Audio/Plug-Ins/HAL/eqMac.driver`. If a connected HDMI/DisplayPort output is visible, `install` also offers to install the Rusty Jack native audio driver. It then writes `~/Library/LaunchAgents/com.example.rusty-jack.plist`, creates `~/Library/Logs`, bootstraps the job in the current user’s launchd domain, and starts `rusty-jack daemon`. The daemon writes structured logs to `~/Library/Logs/rusty-jack.log` (configurable via `logging.file` in config or `RUSTY_JACK_LOG_FILE`).

### ScalarWebAPI speaker wake (interactive install)

On first-time interactive `install` or `config init`, Rusty Jack can configure [`scalar_webapi_device`](#scalar_webapi_device) without hand-editing JSON:

1. **LAN scan** — sends an SSDP/UPnP M-SEARCH for ScalarWebAPI devices on the local network (about 3 seconds when the Mac has a default LAN route).
2. **Device choice** — if one speaker is found, `install` proposes it and pre-fills `host`; if several are found, you pick from the list, enter a host manually, or skip setup; if none are found, `install` offers optional manual setup.
3. **Connection type** — you choose how the speaker is physically connected to this Mac: HDMI/DisplayPort output, headphone/line-out port, USB audio device, or “not sure” (show all selectable outputs).
4. **Mac output** — pick the matching CoreAudio output that should trigger wake (`scalar_webapi_device.mac_output`).
5. **Speaker input** — Rusty Jack queries the speaker for available inputs (for example HDMI, Audio in, Bluetooth) and asks which one matches the Mac connection. The choice is stored as `speaker_input` (human-readable label). Rusty Jack resolves the device URI at runtime and validates the label when the speaker is reachable.
6. **Wake triggers** — confirm the recommended set (`keyboard`, `mouse`, `output_selected`) or toggle individual triggers.

Rusty Jack targets **network speakers** for this flow. TV-class ScalarWebAPI endpoints (for example Bravia displays) are **not** proposed during install discovery even if they answer on the LAN. Re-run interactive `install` later to add ScalarWebAPI to an existing config or use the reconfigure prompts when updating an existing file.

At runtime, wake resolves the JSON-RPC base URL in this order: in-memory cache, fresh on-disk discovery cache, SSDP/UPnP for the configured `host`, a stale on-disk cache entry (unless a non-legacy config `port` disagrees — then config wins), then config `host`/`port`/`path`. Set `port` to the device’s advertised ScalarWebAPI port (often `54480`) so wake still works when SSDP is blocked or flaky.

### Native HDMI/DisplayPort Driver

> **Not yet usable from Homebrew/releases.** Packages include `RustyJack.driver`, but it is ad-hoc signed. macOS AMFI typically rejects it (`signature not valid: -67050`) until the bundle is signed with a **Developer ID Application** identity (and notarized for other Macs). For HDMI/DP volume keys today, use **eqMac** if already installed. Developers: [DRIVER_SIGNING.md](./DRIVER_SIGNING.md).

When the bundled `RustyJack.driver` is signed with a **Developer ID Application** identity, `install`, `picker`, and `upgrade` automatically offer native driver install for connected HDMI/DisplayPort outputs. **Ad-hoc release bundles** still need **eqMac** for HDMI/DP volume keys today. Developers testing unsigned bundles can use `rusty-jack driver swap-in` or set `RUSTY_JACK_OFFER_NATIVE_DRIVER=1` to force install prompts locally.

When user install is enabled, Rusty Jack offers install only when a live HDMI/DP output is present; USB microphones, built-in outputs, Bluetooth, and virtual devices do not trigger the offer. Rusty Jack looks for `RustyJack.driver` in this order:

1. `RUSTY_JACK_DRIVER_BUNDLE`
2. next to the running `rusty-jack` binary
3. `../share/rusty-jack/RustyJack.driver` relative to the binary prefix, which is the Homebrew-style layout

The driver is installed to:

```text
~/Library/Audio/Plug-Ins/HAL/RustyJack.driver
```

This is a per-user HAL plug-in path, so install and removal do not need `sudo`. On **unsigned release bundles**, CoreAudio may never publish the virtual output even though `rusty-jack status` lists the installed path — that is expected until you sign the bundle. After a successful signed install, restart audio apps if they do not immediately see the virtual device. `rusty-jack status` reports scope, path, version, stage, and warnings.

Packages include the bundle at `share/rusty-jack/RustyJack.driver`. Source installs can build the same layout with `make install`, or just validate the packaged bundle with:

```bash
make validate-driver-bundle
```

The current bundle exposes a virtual HAL output named **Rusty Jack**, with a stereo output stream plus volume and mute controls. When the native driver is installed and policy targets HDMI/DisplayPort, `rusty-jack apply` / the daemon sets **Rusty Jack** as the system default, capture mixed audio in a shared ring on `WriteMix`, and render it to the configured physical output with software gain driven by the virtual volume keys (`passthrough-active` stage).

Manual smoke test:

```bash
make install              # source checkout only; packages already include the bundle
make validate-driver-bundle
rusty-jack install
rusty-jack status
rusty-jack uninstall --only-driver
rusty-jack install
```

Check that `rusty-jack status` shows `driver scope: user`, the user HAL path above, a driver version, and `driver stage: passthrough-active`. Audio MIDI Setup or System Settings should show a **Rusty Jack** output after install and stop showing it after `uninstall --only-driver` once CoreAudio refreshes. With the daemon running, logs should include `passthrough: armed for …` and `passthrough: rendering to …` when HDMI/DisplayPort is the policy target. Audible output should come from the physical HDMI/DP device, not from selecting **Rusty Jack** manually in System Settings.

### Pause, Resume, Uninstall

```bash
rusty-jack pause      # stop auto-routing; keep plist installed
rusty-jack resume     # re-enable and start the plist
rusty-jack uninstall  # stop, disable, remove plist, offer driver/config cleanup
rusty-jack uninstall --only-driver  # remove only the native audio driver
rusty-jack uninstall --remove-config  # remove config and purge logs without prompting
rusty-jack uninstall --purge-logs  # remove rotated daemon logs only
rusty-jack uninstall --purge  # config + logs + audio restore (full cleanup)
```

`resume` applies the configured route and volume synchronously, then starts the daemon. If the daemon was paused because `picker` selected a non-preferred output, interactive `resume` first asks whether to return to the configured output; declining leaves the daemon paused. `disable` remains available for daemon-only removal and always keeps `~/.config/rusty-jack/config.json` and log files. `uninstall` prompts before removing the native driver when it is installed, then prompts before removing the default config in interactive mode; `--only-driver` removes only the native driver and leaves the LaunchAgent, binary, config, and logs alone. `--keep-config` keeps config without prompting. `--purge-logs` removes `~/Library/Logs/rusty-jack.log` and rotated `*.log.*` files (also included with `--remove-config` or `--purge`). `--purge` is shorthand for `--remove-config --purge-logs` plus audio restore. `brew uninstall rusty-jack` runs `rusty-jack disable` automatically.

### Update

Replace the binary first, then refresh the LaunchAgent:

```bash
git pull
make upgrade
```

`make upgrade` installs the new binary once, then runs `rusty-jack upgrade --force` so launchd restarts after an in-place source install. The CLI `upgrade` command itself does not download source or build a new binary. It checks the bundled native driver against the installed driver and only offers a driver upgrade when the bundled driver has a material change. It rewrites the plist only when the LaunchAgent differs from the current `rusty-jack` executable, reports the before/after version and commit for real daemon refreshes, and automatically pauses/resumes the daemon if it was running. If the daemon was paused before the upgrade, it stays paused; if the daemon was not installed yet, `upgrade` installs it. Use `--force` to rewrite/restart even when the LaunchAgent already matches — recommended after every Homebrew install/upgrade and after upgrading from releases that used separate launchd stdout/stderr log paths so the plist picks up in-app logging.

After `brew install` or `brew upgrade rusty-jack`, run `rusty-jack upgrade --force` in each macOS user account that uses the daemon. `install`/`upgrade` stamp `RUSTY_JACK_DAEMON_PKG_VERSION` and `RUSTY_JACK_DAEMON_GIT_COMMIT` into the LaunchAgent; `status` reads those env vars from the running daemon PID and flags `daemon stale: yes` with a `note` when they differ from the current CLI.

---

## `list`

Enumerate CoreAudio output devices.

```bash
rusty-jack list
rusty-jack list --hdmi
rusty-jack list --discover
rusty-jack list --json
```

| Flag | Description |
|------|-------------|
| `--hdmi` | Only HDMI, DisplayPort, Thunderbolt, USB |
| `--discover` | Run LAN discovery for ScalarWebAPI speakers and refresh cache for the configured host |
| `--json` | `DeviceList` JSON |

Table columns: **IDX**, **ACT** (`>` = active route), **ALIVE**, **TRANSPORT**, **DEVICE**, **UID**.

Non-selectable devices (aggregates, some virtual apps) may appear dimmed when color is enabled.

`--discover` prints a short footer with LAN discovery count and configured-host cache refresh status. In `--json` mode it adds `scalar_webapi_discovered`.

---

## `scalar-webapi-device`

ScalarWebAPI speaker helpers (subcommands are alphabetical under `scalar-webapi-device`).

### `discover`

Scan the LAN for ScalarWebAPI-compatible speakers.

```bash
rusty-jack scalar-webapi-device discover
rusty-jack scalar-webapi-device discover --json
rusty-jack scalar-webapi-device discover --timeout-ms 5000
```

| Flag | Description |
|------|-------------|
| `--json` | Discovery results as JSON |
| `--timeout-ms N` | SSDP scan timeout (default ~3s) |

Use this after a speaker firmware change or DHCP reassignment to confirm the device is visible on the LAN.

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

Snapshot of devices, virtual system default, policy evaluation, HDMI/DisplayPort volume-control status, and per-user LaunchAgent state.

```bash
rusty-jack status
rusty-jack status --json
rusty-jack status --config ~/.config/rusty-jack/config.json
```

Policy block fields (aligned columns):

- `configured`, `config`, `device`, `preferred`, `active`, `matches`, `auto_switch`
- `config volume`, `volume` (current effective %)
- `note` (human-readable policy message)

HDMI/DisplayPort Volume Control block fields include whether a connected HDMI/DP output is detected, whether the Rusty Jack native driver is installed, whether the driver is recommended for the current hardware, whether eqMac fallback is installed, any managed eqMac backup created by `rusty-jack driver swap-in`, and a recommendation when a connected HDMI/DP route needs volume control.

Daemon block fields include `installed`, `running`, and `paused` booleans, plus the launchd label, service, plist path, PID when available, and the daemon log file path. State values:

- `running` — LaunchAgent plist exists and launchd reports the job loaded; PID is shown when available.
- `paused` — plist exists but launchd does not currently have the job loaded. If picker paused the daemon for a manual output override, `status` includes a `reason` and a note telling you to run `rusty-jack resume`.
- `not_installed` — plist is not present under `~/Library/LaunchAgents`.

When the daemon has run at least one activity poll, an **Activity** block shows the latest idle sample, console and daemon users, configured keyboard/mouse wake triggers, and the last idle→active transition (proxy for recent keyboard/mouse activity). Activity polls log at `debug`; transitions log at `info`.

Config is optional for `status`; without it, policy reports “not configured”.

When ScalarWebAPI is configured, `status` reads power state from cached/configured endpoint details only (no SSDP LAN scan). Run `rusty-jack list --discover` to refresh discovery cache.

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
    "name": "HDMI",
    "uid": "PASTE-UID-FROM-rusty-jack-list"
  },
  "also_set_system_output": true,
  "volume": 13
}
```

### `preferred_device`

Use `uid` as the stable selector. Keep `name` as the human-readable CoreAudio device label:

- `name` — device name from the `list` DEVICE column, emitted for readability
- `uid` — stable CoreAudio UID from `list`, used for routing

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
| `activity_monitor` | `idle` | `idle` (macOS idle time) or `event_tap` (listen-only tap for activity detection; **does not log keystrokes** — see [Event tap activity monitor](#event-tap-activity-monitor); falls back to `idle` when unavailable, disabled, or silent; Accessibility permission required; restart daemon after granting). Interactive ScalarWebAPI install with `keyboard`/`mouse` triggers sets `event_tap` automatically. |
| `activity_active_confirm_ms` | `5000` | Activity must remain below `activity_idle_threshold_ms` for this long before the next sample counts as an idle→active transition. `0` disables confirmation. |
| `activity_event_tap_include_mouse_move` | `false` | When using `event_tap`, count `MouseMoved` events as activity. Leave `false` to reduce Bluetooth pointer jitter false wakes. |

### `volume`

Integer 0–100. Created automatically from the preferred route's current effective volume when `install` can read it. This config value is authoritative for the configured preferred output. Other outputs use per-device remembered volume stored in `~/.config/rusty-jack/device-volumes.json`; Rusty Jack records a non-preferred output's volume before switching away and restores it when switching back. Scheduled no-op polls do not keep forcing volume, so manual volume changes are not fought every poll. Uses retry + readback for HDMI/DisplayPort volume-control compatibility.

### `scalar_webapi_device`

Optional block for waking a ScalarWebAPI-compatible **network speaker** attached to a Mac output. Rusty Jack is not aimed at waking TVs, even though some TVs expose ScalarWebAPI on the LAN. Rusty Jack prefers the SSDP/UPnP-advertised JSON-RPC base URL (`X_ScalarWebAPI_BaseURL`); discovered devices typically advertise a port such as `54480`. When SSDP misses, wake and input ensure fall back to a cached endpoint or config `host`/`port`/`path` (and log a warning). Use [ScalarWebAPI speaker wake (interactive install)](#scalarwebapi-speaker-wake-interactive-install) to create this block on first setup. `status` uses cached discovery metadata (or config host/port/path fallback) so it stays local and does not trigger LAN SSDP scans; use `rusty-jack list --discover` to refresh cache on demand. When enabled and `triggers` includes `output_selected`, `apply`, `picker`, and daemon-initiated output switches send `system.setPowerStatus` when the selected Mac output matches `scalar_webapi_device.mac_output`, the device is not already active, and the Mac reports the speaker host as reachable. When `speaker_input` is set, rusty-jack resolves it to the device URI, reads the active speaker input (`getAvailablePlaybackFunction`), and switches it with `setPlayContent` when it drifts (for example after the speaker falls back to HDMI). The effective label (including the `Audio in` default) is validated against `getCurrentExternalTerminalsStatus` when the speaker is reachable; invalid labels are a hard error listing the inputs the device advertises. `status` shows an `input error` row when validation fails. The daemon re-checks power status and speaker input on scheduled polls while already routed to the configured Mac output. When `triggers` includes `keyboard` or `mouse`, `daemon` keeps the device awake while the Mac is active and wakes it again on idle-to-active transitions if that Mac output is already selected and power status confirms the device is not active. Wake attempts are skipped while the default LAN route is down or when macOS reachability reports the configured host as unreachable; they retry after network recovery (including clearing the activity wake debounce when the network fingerprint changes). `mac_output` may be any Mac output connected to the external device; HDMI/DisplayPort volume control is only involved when that output is an HDMI/DP route. See `config.example.scalar-webapi-device.json`.

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `false` | Enables ScalarWebAPI wake integration. |
| `model` | `ScalarWebAPI device` | Human-readable model hint for docs/logging. |
| `host` | none | Hostname, FQDN, or IP used to filter SSDP discovery responses. Required when enabled. |
| `port` | `10000` | JSON-RPC port used when SSDP/cache miss. Prefer the device-advertised port (for example `54480`); the `10000` default is a legacy placeholder that often refuses connections. |
| `path` | protocol default | ScalarWebAPI base path. Usually omit this unless discovery is unavailable and your device needs an override. |
| `mac_output` | none | Device selector for the Mac output connected to the device. Required when enabled. |
| `triggers` | `["keyboard", "mouse", "output_selected"]` | Wake on explicit output selection and/or while the Mac is active / on idle-to-active transitions. |
| `wake_debounce_ms` | `5000` | Minimum time after a successful `setPowerStatus` or speaker input switch before sending another. Failed sends are retried on the next eligible poll. |
| `speaker_input` | `Audio in` | Human-readable speaker input label from the device (for example `Audio in` on SRS-ZR5 line-in). Omitted or unset values use this default at runtime; an empty string is rejected. Set during interactive `install`. Must match a title from `getCurrentExternalTerminalsStatus` when the speaker is reachable. Legacy alias: `speaker_input_title`. `status` shows `(default)` when the built-in default is in effect. |
| `request_timeout_ms` | `3000` | Network timeout for device requests. |
| `require_quick_start` | `true` | Documents the expectation that the device has its network standby/wake option enabled (e.g. Sony BLUETOOTH/Network standby). |

Other ScalarWebAPI-compatible speakers should work if they expose the same service and advertise an endpoint on your LAN. Example models exercised with Rusty Jack include `SRS-ZR5`, `SRS-ZR7`, `HT-NT5`, `HT-ST5000`, and `STR-DN1080`.

#### ScalarWebAPI references

Sony’s Developer World pages for the Audio Control API / ScalarWebAPI have been archived and may no longer be publicly accessible. These links are still useful:

- **Community forum**: [Sony Developer World forum — Audio Control API](https://techforum.developer.sony.com/category/7/audio-control-api)
- **Archived examples**: [`sonydevworld/audio_control_api_examples`](https://github.com/sonydevworld/audio_control_api_examples)

#### UPnP device description (canonical per-device “documentation”)

In practice, the most accurate reference is whatever your device advertises on your LAN:

- **SSDP search target**: `urn:schemas-sony-com:service:ScalarWebAPI:1`
- **Device description XML** (from SSDP `LOCATION:`) typically includes:
  - `X_ScalarWebAPI_BaseURL` (for example `http://<ip>:10000/sony`)
  - `X_ScalarWebAPI_ServiceList` (service groups like `system`, `audio`, ...)
- **SCPD / action list**: the UPnP service description may reference `ScalarWebApiSCPD.xml` which lists supported actions for that device/firmware.

### Reserved example keys

`match` and `exclude` appear in `config.example.json` as roadmap placeholders and are not applied yet. The `logging` block configures daemon log level and file path (`~/Library/Logs/rusty-jack.log` by default). Override with `RUSTY_JACK_LOG_LEVEL`, `RUSTY_JACK_LOG_FILE`, or `RUST_LOG`.

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
make driver-bundle  # build target/share/rusty-jack/RustyJack.driver
make install    # cargo install --path . and install the bundled driver under ~/.cargo/share
make upgrade    # install once, then force-refresh LaunchAgent
make uninstall  # remove LaunchAgent; prompts before ~/.cargo/bin/rusty-jack (use YES=1 to skip prompt)
```

Cross-compilation targets used in CI: `aarch64-apple-darwin`, `x86_64-apple-darwin`.

`MACOSX_DEPLOYMENT_TARGET=12.0` is set in the Makefile.

### Maintainer release

Publishing to GitHub and the Homebrew tap is documented in [RELEASING.md](./RELEASING.md). Routine releases use `make do-release` from clean `main`; lower-level targets are `make update-release-pr` and `make publish-release`.

---

Copyright (c) 2026 Henrique Andrade / thehcma.
