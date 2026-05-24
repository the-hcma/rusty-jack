# Rusty Jack — Implementation Plan

**Rusty Jack** is a macOS **command-line daemon** (no GUI) that makes **hardware volume keys work** when system audio is routed to an **HDMI, DisplayPort, or dock** output — the same core problem [eqMac](https://github.com/bitgapp/eqMac) solves, without the menu bar app, EQ, or per-app mixer. It lists available external outputs, keeps routing policy-driven, runs under **launchd**, and uses JSON configuration. Written in **Rust**. Installable via **Homebrew** (see [README.md](./README.md)).

This plan is based on investigation of the open-source [eqMac v1.3.2](https://github.com/bitgapp/eqMac) tree (`native/app`, `native/driver`, `native/shared`) and comparable tools ([audio-priority-cli](https://github.com/mateusbadalotti/audio-priority-cli-macos), [audioswitch](https://github.com/retrography/audioswitch)).

---

## 0. Problem statement (why this exists)

### The user-visible bug

When macOS is playing through a **DisplayPort or HDMI monitor** (or many USB-C docks), pressing the **volume keys** (F10 / F11 / F12, Touch Bar, or external keyboard media keys) often **does nothing useful**:

- The on-screen volume overlay may not appear, or
- The slider moves but **audible level does not change**, or
- Volume is stuck at “100%” with no fine control.

Built-in speakers and many Bluetooth/USB headsets expose **software-controllable output volume** in CoreAudio. **External displays frequently do not** — they behave as fixed-gain digital outputs. macOS routes audio directly to them; the system volume control has nothing meaningful to adjust.

### What eqMac actually fixes

eqMac is **not** primarily “pick HDMI instead of built-in.” Its essential trick is:

1. Install a **virtual CoreAudio HAL device** (AudioServerPlugIn driver).
2. Set that virtual device as the **system default output** (what apps and volume keys target).
3. **Capture** system audio in the app, apply **software volume** (and optional EQ), and **re-render** to the real physical device (HDMI/DP/USB).

Volume keys then adjust eqMac’s **software gain** on the virtual device path, which eqMac maps to the physical output — so keyboard volume control works even when the monitor itself is fixed-volume.

### What Rusty Jack aims to do

| Goal | Notes |
|------|--------|
| **Volume keys control audible level on HDMI/DP** | Primary outcome — same class of fix as eqMac |
| **Route to chosen external output** | Required companion: user picks which monitor/dock |
| **Wake Sony speakers on user activity** | When line-out is the active/preferred output and **mouse or keyboard activity** is detected, call Sony **ScalarWebAPI** (`system.setPowerStatus`) to wake an **SRS-ZR5** on the LAN — see §1.1 |
| **No GUI, launchd-friendly, JSON config** | Deliberate simplification vs eqMac |
| **No EQ, booster, or per-app mixer in v1** | Out of scope unless explicitly added later |

**Phased delivery:** Phases 1–6 (below) build **enumeration, routing, config, and daemon** — necessary infrastructure, but **insufficient alone** for working volume keys. **Phase 7+** adds a **virtual output device + software volume pipeline** (eqMac-class architecture, stripped down). Document this clearly so “default output switching only” is not mistaken for the finished product.

---

## 1. Goals and non-goals

### Goals

| Requirement | Approach |
|-------------|----------|
| **Volume keys work on HDMI/DP output** | Virtual HAL device as system default + **software volume** in daemon (eqMac-style passthrough); see §0 and Phase 7 |
| Redirect system sound to HDMI/DP | Set macOS **default output device** via CoreAudio HAL (`kAudioHardwarePropertyDefaultOutputDevice`); virtual device becomes default once driver exists |
| List HDMI outputs | Enumerate output devices; filter by **transport type** and/or UID/name rules |
| launchd integration | Ship a **LaunchAgent** plist + `install-agent` / `uninstall-agent` CLI |
| Periodic inspection + auto-switch | **Property listeners** (event-driven) + **polling fallback** (covers wake/clamshell cases eqMac misses) |
| JSON configuration | `~/.config/rusty-jack/config.json` (path overridable) |
| Homebrew distribution | Native binary via personal tap → optional homebrew-core |
| **Clean uninstall** | `rusty-jack uninstall` stops agent, removes plist, optional config/logs; Brew `uninstall` hook |
| **Intel + Apple Silicon** | Cross-compile both targets; release **universal** binary + per-arch Homebrew bottles |
| **macOS 12+ (Monterey)** | Minimum deployment target; CoreAudio HAL for routing; virtual driver when volume phase ships |
| Rust + best-practice tooling | `rustfmt`, `clippy` (deny warnings in CI), optional `cargo-deny` / `cargo-audit` |
| **Unit tests per component** | Every module has `#[cfg(test)]` coverage; CI runs `cargo test` on macOS; CoreAudio behind traits for mocks |
| **Sony SRS-ZR5 wake on user input** | Map Mac **line-out** UID to ScalarWebAPI endpoint; wake on mouse/keyboard activity via native Rust HTTP client; see §1.1 |

### 1.1 Sony SRS-ZR5 wake-on-user-activity (planned)

#### Problem

A Mac’s **headphone / line-out jack** may be cabled to a **Sony SRS-ZR5** (or similar Songpal speaker) analog input. The speaker often sits in **standby** to save power. When the user returns to the Mac — moves the mouse, clicks, scrolls, or types — audio may be routed to line-out but nothing is audible until someone wakes the speaker manually (remote, Songpal app, etc.).

#### Desired behaviour

1. User configures rusty-jack with:
   - **`preferred_device_uid`** (or equivalent) = the Mac **Built-in Output / line-out** CoreAudio device that feeds the SRS-ZR5.
   - **`sony_speaker`** block = ScalarWebAPI endpoint + model hint for the ZR5 on the LAN.
2. Daemon monitors **user input activity** — **keyboard** (any key down) and/or **mouse** (move, click, scroll) via a macOS event tap.
3. When **both** are true:
   - active or preferred output is the configured line-out UID (or policy has just switched to it), **and**
   - a configured input-activity event occurred (keyboard and/or mouse, per `triggers`),
4. rusty-jack calls the speaker’s **local ScalarWebAPI** (Sony Songpal REST/JSON over HTTP) to **wake** the unit — implemented **natively in Rust**, not via python-songpal.

#### Reference: Sony ScalarWebAPI (Rust-native client)

[python-songpal](https://github.com/rytilahti/python-songpal) is a **protocol reference only** — we do **not** depend on Python, pip, or the `songpal` CLI at runtime. rusty-jack speaks the same HTTP JSON API directly.

| Topic | Detail |
|-------|--------|
| Protocol | Sony **ScalarWebAPI** (“Audio Control API” / Songpal) — JSON-RPC-style POST bodies over **`xhrpost:jsonizer`** (plain HTTP POST) |
| Base URL | `http://<speaker-ip>:10000/sony` (typical for SRS-ZR5; user configures or discovers) |
| Guide endpoint | `{base}/guide` — bootstrap: `getSupportedApiInfo` lists services (`system`, `audio`, `avContent`, …) |
| Service endpoint | `{base}/{service}` — e.g. `http://192.168.1.42:10000/sony/system` |
| Request shape | `{"method":"<name>","params":[{...}],"id":<n>,"version":"1.0"}` with `Content-Type: application/json` |
| Method discovery | POST `getMethodTypes` with `params: [""]` on each service URL (see python-songpal `Service.fetch_signatures`) |
| **Wake (Phase 8 minimum)** | `system.setPowerStatus` with `params: [{"status":"active"}]` |
| **Status (optional)** | `system.getPowerStatus` — skip wake if already active; log standby vs off |
| WebSocket | Used by python-songpal for push notifications (`notifyPowerStatus`, etc.) — **optional later** in Rust; not required for wake-on-user-activity |
| SRS-ZR5 | Listed as officially supported by Sony’s Songpal / Home Assistant integration |
| Power on | **`Quick Start-Up`** must be enabled on the speaker for network wake from standby; cold power-off may need **Wake-on-LAN** (future; MAC from `getSystemInformation`) |
| Rust deps | `reqwest` (+ `serde_json`) for HTTP; mock with `wiremock` or httptest in unit tests |
| Isolation | `SpeakerWake` trait + `ScalarWebClient` + `MockSpeakerWake`; no LAN I/O in default unit tests |

##### ScalarWebAPI call flow (wake)

```text
1. POST {endpoint}/guide
   {"method":"getSupportedApiInfo","params":[{}],"id":1,"version":"1.0"}
   → confirms "system" service exists

2. POST {endpoint}/system
   {"method":"getPowerStatus","params":[{}],"id":2,"version":"1.0"}
   → if already "active", no-op (respect debounce)

3. POST {endpoint}/system
   {"method":"setPowerStatus","params":[{"status":"active"}],"id":3,"version":"1.0"}
   → wake from standby
```

Example (curl):

```bash
curl -s -X POST "http://192.168.1.42:10000/sony/system" \
  -H "Content-Type: application/json" \
  -d '{"method":"setPowerStatus","params":[{"status":"active"}],"id":1,"version":"1.0"}'
```

##### Rust module layout (planned)

```text
src/sony/
  mod.rs
  scalar_api.rs    # HTTP transport, request envelope, error mapping
  power.rs         # getPowerStatus / setPowerStatus
  discover.rs      # optional SSDP/UPnP endpoint discovery (Phase 8.1)
  traits.rs        # SpeakerWake trait
src/activity/
  mod.rs
  macos.rs         # CGEventTap: keyboard + mouse activity (Phase 8)
  traits.rs        # UserActivityMonitor trait + mock
```

**Non-goal:** shipping or invoking python-songpal, pip, or a Python interpreter.

#### Trigger design (macOS)

```mermaid
flowchart LR
    Input[Keyboard or mouse activity] --> Gate{Preferred output\n== line-out UID?}
    Policy[Policy switched to line-out] --> Gate
    Gate -->|yes| ScalarAPI[ScalarWebAPI setPowerStatus]
    Gate -->|no| Skip[No-op]
    ScalarAPI --> ZR5[SRS-ZR5 analog input]
    MacOut[Mac line-out] --> ZR5
```

**Recommended triggers (configurable):**

| Trigger | Source | Notes |
|---------|--------|-------|
| `keyboard` | `CGEventTap` — `kCGEventKeyDown` (and optionally `kCGEventFlagsChanged` for modifiers) | Primary; any key press counts as “user at desk”; **do not log keycodes** |
| `mouse` | `CGEventTap` — `kCGEventMouseMoved`, button down/up, scroll wheel | Wake on pointer motion, click, or scroll |
| `output_selected` | `kAudioHardwarePropertyDefaultOutputDevice` → configured UID | Wake when output switches to line-out even without input yet |
| `debounce` | Timer in daemon | Avoid spamming the speaker API (e.g. 30–60 s cooldown while already awake) |

**Permissions:** A global event tap requires **Accessibility** (System Settings → Privacy & Security → Accessibility). Document clearly. The tap observes **event types only** for wake gating — not keystroke logging or screen recording.

#### Configuration sketch (see `config.example.json`)

```json
"sony_speaker": {
  "enabled": true,
  "model": "SRS-ZR5",
  "endpoint": "http://192.168.1.42:10000/sony",
  "mac_output_uid": "AppleHDAEngineOutput:…",
  "triggers": ["keyboard", "mouse", "output_selected"],
  "wake_debounce_ms": 30000,
  "request_timeout_ms": 3000,
  "require_quick_start": true
}
```

- **`mac_output_uid`** — CoreAudio UID from `rusty-jack list` for the jack feeding the ZR5 (often `Built-in Output` / `AppleHDAEngineOutput:…`).
- **`endpoint`** — ScalarWebAPI base URL (`http://host:port/sony`); set manually, via DHCP reservation, or `rusty-jack sony discover` (SSDP). Overridable via `RUSTY_JACK_SONY_ENDPOINT`.
- **`request_timeout_ms`** — HTTP timeout for ScalarWebAPI calls.

#### Non-goals for this feature

- Full Songpal remote (EQ, multi-room grouping, source switching on the Sony unit).
- Replacing the Sony app for everyday control.
- Waking speakers when line-out is **not** the active/preferred output.

#### Phase

**Phase 8** (after daemon + config exist — Phase 4+); can ship **before** Phase 7 virtual driver if line-out routing alone is enough for the ZR5 use case.

### Non-goals (for v1)

- Menu bar / web UI (eqMac’s Angular UI is out of scope)
- System-wide EQ, volume booster, per-app mixer (eqMac Pro features)
- Code signing / notarization automation (document manually; required for wide distribution and **required before virtual driver install** for most users)

### Important scope note vs eqMac

eqMac solves **two** coupled problems:

1. **Volume control** — virtual device intercepts volume keys; app applies software gain before the physical output.
2. **Output selection** — user (or heuristics) picks which physical device receives the re-rendered stream.

Tools like [audio-priority-cli](https://github.com/mateusbadalotti/audio-priority-cli-macos) only solve **(2)** — switching the default output UID. That **does not** restore keyboard volume on fixed-gain HDMI/DP displays.

**Rusty Jack** targets eqMac’s **(1) + (2)** with a CLI-first, no-EQ design:

- **Phases 1–6:** Routing, listing, config, daemon — foundation only.
- **Phase 7+:** Virtual AudioServerPlugIn + passthrough with software volume — **required for the core value proposition.**

We intentionally defer the driver (higher complexity, install/uninstall, signing) but **do not** treat it as optional forever; it is how volume keys get fixed.

---

## 2. eqMac architecture (what we learned)

### 2.1 Component diagram

```mermaid
flowchart TB
    subgraph macOS["macOS CoreAudio HAL"]
        Apps[Applications]
        VirtDev["eqMac virtual device (driver)"]
        PhysDev["Physical outputs: Built-in, HDMI, USB, BT..."]
    end

    subgraph eqMacApp["eqMac app (Swift)"]
        Engine["Engine: AVAudioEngine capture + EQ"]
        Output["Output: AVAudioEngine render to selected device"]
        Events["AudioDeviceEvents: AMCoreAudio notifications"]
        OutputsMod["Outputs: device filter + transport types"]
    end

    subgraph eqMacDriver["eqMac driver (Swift ASPL)"]
        RingBuf["Ring buffer / IO in EQMDevice"]
    end

    Apps --> VirtDev
    VirtDev --> RingBuf
    RingBuf --> Engine
    Engine --> Output
    Output --> PhysDev
    Events --> OutputsMod
    OutputsMod --> Output
```

### 2.2 Relevant eqMac source locations

| Area | Path | Role |
|------|------|------|
| Device filtering | `native/app/Source/Audio/Outputs/Outputs.swift` | `SUPPORTED_TRANSPORT_TYPES` includes `.hdmi`, `.displayPort`, `.thunderbolt`, `.usb`, etc.; excludes driver UID and aggregate devices |
| Auto-select heuristic | `Outputs.shouldAutoSelect` | Auto-picks **bluetooth / built-in** on plug-in — **not** HDMI (HDMI selection is user-driven or UI-driven) |
| Default output change | `Application.startPassthrough` | Sets `AudioDevice.currentOutputDevice = Driver.device!` (virtual), stores real target in `selectedDevice` |
| Physical routing | `Application.createAudioPipeline` | `Output(device: selectedDevice!)` renders processed audio to hardware |
| Hot-plug | `Application.setupDeviceEvents` | `deviceListChanged`, `outputChanged`, `isJackConnectedChanged` |
| CoreAudio helpers | `native/app/Source/Extensions/AudioDevice.swift` | UID lookup, `setAsDefaultOutputDevice()`, volume/balance |
| Virtual driver | `native/driver/Source/EQM*.swift` | AudioServerPlugIn: properties, ring buffer, sample rates |
| UI transport types | `ui/src/app/sections/outputs/outputs.service.ts` | `'hdmi' \| 'displayPort' \| ...` exposed to Angular UI |

### 2.3 eqMac behaviors to copy or improve

**Copy (concepts):**

- **Virtual device as default output** so volume keys target controllable software gain, not the fixed HDMI endpoint.
- Filter devices by `kAudioDevicePropertyTransportType` (HDMI = `'hdmi'`, DisplayPort often carries display audio too).
- Use stable **`kAudioDevicePropertyDeviceUID`** in config (names change; IDs can change across reboots but UIDs are the usual key).
- Listen for `kAudioHardwarePropertyDevices` list changes and `kAudioHardwarePropertyDefaultOutputDevice` changes.
- Exclude virtual/aggregate/driver devices from user-facing lists (eqMac excludes `Constants.DRIVER_DEVICE_UID`, `CADefaultDeviceAggregate`).
- **Software volume / passthrough pipeline** — capture from virtual device, apply gain from system volume property, render to selected physical UID (`Output.swift`, `Engine.swift`).

**Improve (known eqMac pain points):**

- [Issue #829](https://github.com/bitgapp/eqMac/issues/829): auto-switch fails after sleep/clamshell when HDMI was plugged in while lid closed. Mitigate with:
  - **Polling** on interval (configurable) in addition to listeners.
  - **Wake notifications** (`NSWorkspace.didWakeNotification` equivalent — in a CLI daemon, use `IORegisterForSystemPower` or periodic re-enumeration after resume).
  - Optional **delay after device list change** before applying switch (eqMac uses `Async.delay(500)` / `1000` ms in several paths).

---

## 3. Recommended architecture for this project

### 3.1 High-level design

```mermaid
flowchart LR
    subgraph daemon["rusty-jack daemon"]
        CLI[CLI subcommands]
        CFG[Config JSON]
        Enum[Device enumerator]
        Pol[Policy engine]
        HAL[CoreAudio HAL wrapper]
        Loop[Run loop: listeners + poll timer]
    end

    subgraph launchd["launchd LaunchAgent"]
        Plist[plist: KeepAlive, RunAtLoad]
    end

    Plist --> daemon
    CFG --> Pol
    Enum --> Pol
    Pol --> HAL
    HAL --> macOS["CoreAudio"]
    Loop --> Pol
```

**Single binary**, two modes:

1. **Foreground / one-shot CLI** — `list`, `status`, `set`, `apply`, `install-agent`, etc.
2. **Daemon mode** — `rusty-jack daemon` (long-running; used by launchd)

**Phase 1–6:** HAL-only binary (enumerate + route to physical device). **Phase 7+:** daemon also hosts **passthrough + software volume** once the virtual driver is installed; system default becomes the Rusty Jack virtual device.

### 3.2 Crate layout

```
rusty-jack/
├── Cargo.toml
├── rustfmt.toml
├── clippy.toml                    # optional: warn = deny for pedantic subset
├── .github/workflows/ci.yml       # fmt, clippy, test, both targets
├── .github/workflows/release.yml  # aarch64 + x86_64 + universal tarball
├── README.md
├── IMPLEMENTATION_PLAN.md         # this file
├── config.example.json
├── .cargo/config.toml             # MACOSX_DEPLOYMENT_TARGET=12.0
├── scripts/build-universal.sh     # aarch64 + x86_64 → lipo
├── packaging/homebrew/rusty-jack.rb
├── launchd/
│   └── com.example.rusty-jack.plist.template
├── src/
│   ├── main.rs                    # clap entry, subcommands
│   ├── lib.rs
│   ├── config.rs                  # JSON schema + load/save
│   ├── error.rs
│   ├── coreaudio/
│   │   mod.rs
│   │   device.rs                  # enumerate, metadata, transport type
│   │   default_output.rs          # get/set default output + system output
│   │   listener.rs                # property listeners + run loop integration
│   │   sys.rs                     # unsafe FFI wrappers (thin)
│   ├── policy.rs                  # “should switch?” + target selection
│   ├── sony/                      # Phase 8: ScalarWebAPI client (SRS-ZR5 wake)
│   │   mod.rs
│   │   scalar_api.rs
│   │   power.rs
│   │   discover.rs
│   │   traits.rs
│   ├── activity/                  # Phase 8: keyboard + mouse activity monitor
│   │   mod.rs
│   │   macos.rs
│   │   traits.rs
│   ├── daemon.rs                  # main loop, poll, wake handling
│   ├── launchd.rs                 # install/uninstall agent plist
│   └── cli.rs                     # clap parsing (testable without main)
└── tests/
    ├── cli_integration.rs         # optional: assert_cmd end-to-end
    └── fixtures/                  # JSON configs, plist golden files, device snapshots
```

Each `src/*.rs` and `src/coreaudio/*.rs` module includes a `#[cfg(test)] mod tests { ... }` block (or `tests/<module>_tests.rs` only for cross-module integration). **No component ships without unit tests.**

### 3.3 Dependencies (Rust)

| Crate | Purpose |
|-------|---------|
| `coreaudio-rs` **or** `objc2-core-audio` | CoreAudio HAL bindings (prefer whichever exposes `AudioObjectSetPropertyData` + transport type cleanly) |
| `clap` (derive) | CLI |
| `serde`, `serde_json` | Config |
| `thiserror`, `anyhow` | Errors (`thiserror` in library, `anyhow` in binary) |
| `tracing`, `tracing-subscriber` | Structured logs (JSON or pretty in foreground) |
| `directories` | Default config path under `~/.config` |
| `reqwest` | Phase 8: Sony ScalarWebAPI HTTP client (blocking or async) |
| `core-graphics` | Phase 8: `CGEventTapCreate` for keyboard/mouse activity monitor |
| `ctrlc` | Graceful shutdown in daemon mode |

**Dev-dependencies (tests):**

| Crate | Purpose |
|-------|---------|
| `tempfile` | Isolated config/state/plist paths in unit tests |
| `pretty_assertions` | Readable diffs for policy/plist golden tests |
| `assert_cmd` / `predicates` | Optional CLI integration tests in `tests/` |
| `wiremock` | Phase 8: mock ScalarWebAPI HTTP responses in unit tests |
| `serial_test` | Optional: serialize tests that touch global HAL mocks |

**macOS-only:** gate with `cfg(target_os = "macos")` and fail compile on other targets with a clear message.

### 3.4 CoreAudio operations (implementation detail)

#### Enumerate output devices

1. `AudioObjectGetPropertyData` on `kAudioObjectSystemObject` with `kAudioHardwarePropertyDevices`.
2. For each `AudioDeviceID`, check output capability (`kAudioDevicePropertyStreamConfiguration`, scope output).
3. Read properties:
   - `kAudioDevicePropertyDeviceUID` (CFString → Rust `String`)
   - `kAudioObjectPropertyName`
   - `kAudioDevicePropertyTransportType` (`UInt32` FourCC)
   - `kAudioDevicePropertyDeviceIsAlive`
   - Optional: `kAudioDevicePropertyDataSource` / name for multi-source devices (eqMac uses `sourceName` in JSON)

#### Identify “HDMI outputs”

Default filter (configurable):

| Transport FourCC | Typical hardware |
|----------------|------------------|
| `'hdmi'` | HDMI audio |
| `'dp  '` / displayPort | DisplayPort audio |
| `'thun'` / thunderbolt | Dock / monitor over TB |
| `'usb '` | USB-C docks (optional, often user wants this) |

Also support **explicit UID allowlist** and **name substring** rules in JSON for edge cases (CalDigit, “LG TV”, etc.).

#### Set default output

```text
selector: kAudioHardwarePropertyDefaultOutputDevice
scope:    kAudioObjectPropertyScopeGlobal
element:  kAudioObjectPropertyElementMain  (alias Master on older macOS)
object:   kAudioObjectSystemObject
data:     AudioDeviceID
```

Optionally also set `kAudioHardwarePropertyDefaultSystemOutputDevice` so alert sounds match (eqMac sets both virtual driver paths; for physical-only routing, setting both to the same HDMI device is reasonable).

**macOS 15+:** Apple introduced higher-level `AudioHardwareSystem` APIs in Swift; Rust should stay on HAL C API for broad OS support unless you add a thin Swift shim (not recommended for v1).

#### Event-driven monitoring

Register `AudioObjectAddPropertyListener` on:

- `kAudioHardwarePropertyDevices` (list changes)
- `kAudioHardwarePropertyDefaultOutputDevice` (something else changed output)
- Per-device: `kAudioDevicePropertyDeviceIsAlive` for tracked UIDs

**Run loop requirement:** On macOS 10.6+, set `kAudioHardwarePropertyRunLoop` on the system object to the thread’s `CFRunLoop` (or `NULL` for synchronous delivery on listener thread — eqMac/AMCoreAudio uses Foundation run loop). Daemon thread should call `CFRunLoopRun` or integrate with `core-foundation` crate.

#### Polling fallback

Configurable `poll_interval_ms` (default e.g. `3000`). Each tick:

1. Re-enumerate devices.
2. Run policy: if preferred HDMI is present and default ≠ target, call set default.
3. Log at `debug` when no-op; `info` when switching.

This directly addresses eqMac-style missed events after wake/dock hot-plug.

---

## 4. JSON configuration

**Default path:** `~/.config/rusty-jack/config.json`

### 4.1 Schema (example)

```json
{
  "version": 1,
  "auto_switch": true,
  "poll_interval_ms": 3000,
  "switch_delay_ms": 500,
  "preferred_device_uid": "HDMI-XXXX-UID-FROM-LIST-COMMAND",
  "fallback_uids": [
    "DisplayPort-Secondary-UID"
  ],
  "match": {
    "transport_types": ["hdmi", "displayport", "thunderbolt"],
    "name_contains": [],
    "uid_allowlist": []
  },
  "exclude": {
    "name_contains": ["eqMac", "CADefaultDeviceAggregate"],
    "uid_denylist": []
  },
  "also_set_system_output": true,
  "sony_speaker": {
    "enabled": false,
    "model": "SRS-ZR5",
    "endpoint": "http://192.168.1.42:10000/sony",
    "mac_output_uid": "PASTE-LINE-OUT-UID-FROM-rusty-jack-list",
    "triggers": ["keyboard", "mouse", "output_selected"],
    "wake_debounce_ms": 30000,
    "request_timeout_ms": 3000,
    "require_quick_start": true
  },
  "logging": {
    "level": "info",
    "file": "~/Library/Logs/rusty-jack.log"
  }
}
```

### 4.2 Field semantics

| Field | Description |
|-------|-------------|
| `preferred_device_uid` | Primary target; if connected and alive, switch to it when `auto_switch` is true |
| `fallback_uids` | Ordered list tried when preferred is absent |
| `match.transport_types` | Filter for `list --hdmi-only` and optional auto-discovery |
| `match.uid_allowlist` | If non-empty, **only** these UIDs are candidates for auto-switch |
| `auto_switch` | Master enable for daemon behavior |
| `poll_interval_ms` | Polling interval; `0` disables poll (listeners only — not recommended) |
| `switch_delay_ms` | Debounce after device list change before applying (eqMac uses 500–1000 ms) |
| `also_set_system_output` | Mirror alerts/sound effects device |
| `sony_speaker.enabled` | Master switch for SRS-ZR5 / ScalarWebAPI wake logic |
| `sony_speaker.endpoint` | ScalarWebAPI base URL (`http://host:port/sony`) |
| `sony_speaker.mac_output_uid` | CoreAudio UID of Mac line-out wired to the speaker |
| `sony_speaker.triggers` | `keyboard`, `mouse`, `output_selected` (see §1.1) |
| `sony_speaker.wake_debounce_ms` | Minimum interval between wake commands |
| `sony_speaker.request_timeout_ms` | HTTP timeout for ScalarWebAPI POST calls |
| `sony_speaker.require_quick_start` | If true, log a warning when wake fails (user must enable Quick Start-Up on the ZR5) |

### 4.3 Config discovery

Resolution order:

1. `--config /path/to/config.json`
2. `$HDMI_SOUND_CONTROLLER_CONFIG`
3. `~/.config/rusty-jack/config.json`

`config init` writes a starter file with commented JSON or separate `config.example.json` in repo.

---

## 5. CLI specification

Binary name: **`rusty-jack`** (crate `rusty-jack`, `RUSTY_JACK_CONFIG` env override).

| Subcommand | Description |
|------------|-------------|
| `list` | Print output devices (table: index, uid, name, transport, alive, default marker) |
| `list --hdmi` | Only HDMI-matched transports |
| `status` | Current default output + whether it matches policy |
| `set <uid\|index>` | One-shot switch default output |
| `apply` | Apply policy once (useful for scripts) |
| `daemon` | Run supervisor loop (launchd invokes this) |
| `config init` | Write example config |
| `config validate` | Parse and validate JSON |
| `agent install` | Copy plist to `~/Library/LaunchAgents/`, `launchctl bootstrap` |
| `agent uninstall` | Stop job (`bootout`), remove plist only |
| `agent status` | Whether job is loaded |
| **`uninstall`** | **Full cleanup** (see §8): agent + optional config/state/logs + optional audio restore |

**Global flags:** `--config`, `-v` / `--verbose`, `--json` for machine-readable `list`/`status`.

### `uninstall` flags

| Flag | Default | Effect |
|------|---------|--------|
| `--purge` | off | Remove config, state dir, and log files |
| `--keep-config` | off | Keep `~/.config/rusty-jack/` (mutually exclusive with `--purge`) |
| `--restore-audio` | on | Restore default output UID saved before first switch (if state exists) |
| `--no-restore-audio` | — | Leave current system output unchanged |
| `-y` / `--yes` | off | Non-interactive (for Homebrew `uninstall` hook) |

Idempotent: safe to run twice; second run reports “nothing to do” where appropriate.

### Example session

```bash
# Discover devices
rusty-jack list --hdmi

# Set preferred UID in config, then
rusty-jack config init
$EDITOR ~/.config/rusty-jack/config.json

# Test once
rusty-jack apply

# Install background service
rusty-jack agent install
rusty-jack agent status

# Or install the binary via Homebrew first
brew install YOUR_USER/tap/rusty-jack

# Remove everything later
rusty-jack uninstall --purge -y
# or: brew uninstall rusty-jack   # formula calls uninstall hook
```

---

## 6. Platform support (macOS 12+, Intel & ARM)

### Minimum macOS version: **12.0 Monterey**

Target machines include **older Intel Macs** and **Apple Silicon**, e.g. macOS 12.7 on Intel (darwin 21.x). Rationale:

| Factor | Choice |
|--------|--------|
| APIs | `AudioObjectGetPropertyData` / `SetPropertyData` for default output — stable since 10.4; use `kAudioObjectPropertyElementMain` with fallback to `Master` if needed |
| No kernel/driver | Avoids HAL plug-in install complexity and SIP/driver signing on old OS |
| `launchctl bootstrap` | Supported on Monterey; prefer over deprecated `load` |
| Rust | Pin `MACOSX_DEPLOYMENT_TARGET=12.0` in `.cargo/config.toml` and CI |

**Not supported in v1:** macOS 11 and earlier (possible later if demand exists; would require broader QA matrix).

### Architecture matrix

| Artifact | Intel (`x86_64-apple-darwin`) | Apple Silicon (`aarch64-apple-darwin`) |
|----------|-------------------------------|----------------------------------------|
| GitHub Release tarball | `rusty-jack-{version}-x86_64-apple-darwin.tar.gz` | `rusty-jack-{version}-aarch64-apple-darwin.tar.gz` |
| Optional universal | `rusty-jack-{version}-universal-apple-darwin.tar.gz` via `lipo` | Same binary runs native on both with Rosetta **not** required per arch build |
| Homebrew bottle | Built for host arch at install time, or prebuilt bottle per arch | Same |

Rosetta: ship **native** binaries for each arch; do not rely on Rosetta for the daemon.

---

## 7. Cross-compilation and release builds

### Toolchain

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

`.cargo/config.toml` (project-wide):

```toml
[env]
MACOSX_DEPLOYMENT_TARGET = "12.0"

[target.aarch64-apple-darwin]
rustflags = ["-C", "link-arg=-mmacosx-version-min=12.0"]

[target.x86_64-apple-darwin]
rustflags = ["-C", "link-arg=-mmacosx-version-min=12.0"]
```

### Local / CI build commands

```bash
export MACOSX_DEPLOYMENT_TARGET=12.0

# Per architecture
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

# Universal binary (releases / manual install)
lipo -create \
  target/aarch64-apple-darwin/release/rusty-jack \
  target/x86_64-apple-darwin/release/rusty-jack \
  -output target/release/rusty-jack-universal
```

Provide `scripts/build-universal.sh` that runs both builds, `lipo`, and optional `codesign` (adhoc for local dev).

### CI (GitHub Actions)

`release.yml` matrix on `macos-13` or `macos-14` runner:

1. Install Rust stable + both targets.
2. Build `aarch64-apple-darwin` and `x86_64-apple-darwin` release binaries.
3. Upload arch-specific tarballs + universal asset.
4. Run `cargo test` on host arch (unit tests); tag manual “audio matrix” for hardware.

`ci.yml` on every PR: `fmt`, `clippy`, `test`, and **compile both targets** (`cargo build --target ...`) to catch cross-compile breakage.

### Verify on old hardware

Before each release, smoke-test on at least:

- [ ] macOS 12.x **Intel** (your machine class)
- [ ] macOS 13+ **Apple Silicon**
- [ ] macOS 14+ **Apple Silicon** (optional)

Check: `list`, `apply`, `agent install` / `uninstall`, sleep/wake + HDMI dock.

---

## 8. Homebrew distribution

Rust compiles to a **native Mach-O binary** per architecture (`aarch64-apple-darwin`, `x86_64-apple-darwin`). Homebrew builds or bottles each arch separately; releases may also ship a **universal** tarball for manual install.

### Recommended path

1. **Personal tap** (`brew tap you/rusty-jack`) with `packaging/homebrew/rusty-jack.rb`.
2. **GitHub Releases** — CI builds release binaries; formula uses `url` + `sha256` or official bottles.
3. **Source formula** (early days) — `depends_on "rust" => :build` + `cargo install` (template in repo).
4. **homebrew-core** (later) — optional; macOS-only tools use `depends_on :macos`.

### User flow

```bash
brew install your/tap/rusty-jack
rusty-jack list --hdmi
rusty-jack config init
rusty-jack agent install
```

Homebrew puts the binary in `$(brew --prefix)/bin` (Apple Silicon: `/opt/homebrew/bin`). `agent install` should use `std::env::current_exe()` when writing the LaunchAgent plist.

### Clean uninstall from Homebrew

The formula **must** invoke the CLI uninstall hook so no LaunchAgent or state is left behind:

```ruby
def uninstall
  # Non-interactive full cleanup when binary still exists
  safe_system bin/"rusty-jack", "uninstall", "--yes", "--purge", "--no-restore-audio"
end
```

Use `--no-restore-audio` in the Brew hook to avoid surprising users who uninstalled the tool but changed outputs since install; document that `rusty-jack uninstall --restore-audio` is available for manual runs before removal.

Optional: print caveats on `brew install` reminding users to run `rusty-jack agent install` after install.

### Notarization

Not required for typical Homebrew installs. Notarize only if you also ship a standalone `.dmg` outside Brew.

---

## 9. Clean uninstall (design)

Uninstall must leave the Mac in a predictable state — no orphaned launchd job, no stale plists, no silent background process.

### What gets removed

| Component | `agent uninstall` | `uninstall` (default) | `uninstall --purge` |
|-----------|-------------------|------------------------|---------------------|
| `launchctl bootout` + stop daemon | yes | yes | yes |
| `~/Library/LaunchAgents/com.*.rusty-jack.plist` | yes | yes | yes |
| `~/.config/rusty-jack/` | no | no | yes |
| `~/.local/state/rusty-jack/` (saved default UID, install metadata) | no | yes | yes |
| `~/Library/Logs/rusty-jack*.log` | no | no | yes |
| Restore previous default output | no | yes (if state exists) | optional (`--no-restore-audio`) |

### State file (for restore)

On **first successful switch** away from the pre-Rusty-Jack default, write once:

`~/.local/state/rusty-jack/pre_install_default.json`

```json
{
  "output_device_uid": "BuiltInSpeakerDeviceUID",
  "saved_at": "2026-05-24T12:00:00Z"
}
```

`uninstall --restore-audio` sets `kAudioHardwarePropertyDefaultOutputDevice` back to that UID if the device still exists; otherwise print a clear warning and list `rusty-jack list`.

### Uninstall algorithm

```text
uninstall(opts):
  1. agent_uninstall()           # bootout, delete plist, SIGTERM if PID known
  2. if opts.restore_audio: restore_from_state()
  3. if opts.purge: remove config, state, logs
     else if not opts.keep_config: remove state only (keep config by default)
  4. print summary of removed paths
```

### Idempotency and errors

- Missing plist → success (already uninstalled).
- `bootout` fails with “not loaded” → treat as success.
- Log failures at `warn` but exit 0 if agent is definitely stopped (user can delete plist manually).

### User-facing messages

End with one line:

`Rusty Jack uninstalled. LaunchAgent removed. Config kept at ~/.config/rusty-jack (use --purge to delete).`

---

## 10. launchd integration

### 10.1 LaunchAgent template

Path: `~/Library/LaunchAgents/com.<reverse-domain>.rusty-jack.plist`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.example.rusty-jack</string>
    <key>ProgramArguments</key>
    <array>
        <string>/opt/homebrew/bin/rusty-jack</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/SHARED/Library/Logs/rusty-jack.stdout.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/SHARED/Library/Logs/rusty-jack.stderr.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
</dict>
</plist>
```

`agent install` should:

1. Resolve absolute path to the binary (`std::env::current_exe` at install time).
2. Substitute user home for log paths.
3. Run `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/....plist` (macOS 12+).

`agent uninstall` runs `launchctl bootout gui/$(id -u) <label>` then deletes the plist.

### 10.2 Permissions

- **No root required** for LaunchAgent or setting default output to a **physical** device.
- **Root (or admin password) required** to install the virtual HAL plugin in `/Library/Audio/Plug-Ins/HAL/` (Phase 7).
- Volume keys are handled via the **virtual device + software gain** path — no Accessibility permission or key simulation needed.

---

## 11. Policy engine (auto-switch logic)

Pseudocode:

```text
function desired_device(enumerated, config) -> Option<Device>:
    if config.preferred_uid is connected and alive:
        return that device
    for uid in config.fallback_uids:
        if uid connected and alive:
            return that device
    if config.match.uid_allowlist non-empty:
        return first connected device in allowlist
    return first connected device matching transport_types filter

function should_switch(current_default, desired, config) -> bool:
    if not config.auto_switch: return false
    if desired is None: return false
    if current_default.uid == desired.uid: return false
    if user_override_grace_period active: return false  # optional future
    return true
```

**Optional v1.1:** `manual_until` timestamp in config or separate state file when user runs `set` manually (don’t fight user for N minutes).

---

## 12. Rust tooling and quality bar

### 12.1 Formatter and linter

`rustfmt.toml`:

```toml
edition = "2021"
max_width = 100
```

CI:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
```

### 12.2 Additional checks (recommended)

| Tool | Purpose |
|------|---------|
| `cargo-deny` | License / advisory DB for dependencies |
| `cargo-audit` | Security advisories |
| `cargo-llvm-cov` | Coverage on policy + config parsing |

### 12.3 MSRV and Xcode SDK

- **Rust:** 1.85+ (pin in `rust-toolchain.toml`).
- **macOS deployment target:** 12.0 (`MACOSX_DEPLOYMENT_TARGET`).
- **CI Xcode:** macOS 12 SDK minimum via runner + deployment target flags (build on `macos-13`+ runners for reliable cross-target support).

### 12.4 Unsafe code

Keep FFI in `coreaudio/sys.rs`; document safety invariants for listener callbacks (Send/Sync — listeners run on audio thread; use channel to policy thread).

### 12.5 Unit test policy (required)

1. **One test module per component** — colocated `#[cfg(test)] mod tests` in the same file as the code under test.
2. **No hardware in unit tests** — CoreAudio I/O goes behind traits (`AudioHal`, `DeviceEnumerator`); unit tests use fakes/fixtures.
3. **PR gate** — `cargo test --all-targets` must pass on macOS CI before merge.
4. **Coverage target (soft)** — ≥80% line coverage on `config`, `policy`, `launchd`, `daemon` logic (exclude `coreaudio/sys.rs` FFI glue); track with `cargo llvm-cov`.
5. **Naming** — `test_<function>_<scenario>` (e.g. `test_policy_prefers_hdmi_when_connected`).

---

## 13. Implementation phases

### Phase 0 — Project bootstrap (0.5 day)

- [ ] `cargo init` workspace, CI, `rustfmt` / `clippy`
- [ ] macOS-only `cfg` gates; `MACOSX_DEPLOYMENT_TARGET=12.0` in `.cargo/config.toml`
- [ ] `tracing` setup
- [ ] `release.yml`: build `aarch64-apple-darwin` + `x86_64-apple-darwin` on every tag
- [ ] **Tests:** `error.rs` unit tests; `cli.rs` parse tests for all subcommands; CI runs `cargo test`

### Phase 1 — CoreAudio read path (1–2 days)

- [ ] `AudioHal` trait + `CoreAudioHal` impl + `MockHal` for tests
- [ ] Enumerate output devices (UID, name, transport, alive)
- [ ] `list` and `status` subcommands
- [ ] **Tests:** `coreaudio/device.rs` — filter HDMI, exclude aggregates, empty list; `default_output.rs` — get default from mock; parse transport FourCC

### Phase 2 — Write path (1 day)

- [ ] `set` / `apply` — set default output (+ optional system output)
- [ ] Manual hardware smoke test (not unit tests)
- [ ] **Tests:** `default_output.rs` — set default calls mock once, handles errors; save pre-install UID on first switch

### Phase 3 — Config + policy (1 day)

- [ ] JSON config load/validate
- [ ] `config init` / `config validate`
- [ ] Policy engine + `apply` respects `preferred_device_uid` / fallbacks
- [ ] **Tests:** `config.rs` — round-trip, missing fields, invalid JSON, path resolution; `policy.rs` — table-driven cases (preferred present/absent, fallbacks, allowlist, `auto_switch: false`)

### Phase 4 — Daemon + listeners + poll (2–3 days)

- [ ] Property listeners + run loop thread
- [ ] Poll timer with debounce (`switch_delay_ms`)
- [ ] `daemon` subcommand; signal handling
- [ ] Wake / relist heuristic (poll burst after `sleep` wake if detectable)
- [ ] **Tests:** `daemon.rs` — debounce timing with mock clock; poll tick invokes policy; listener event → policy (channel injection); `coreaudio/listener.rs` — address registration logic with mock callback dispatch

### Phase 5 — launchd + uninstall (1 day)

- [ ] Plist template + `agent install` / `agent uninstall` / `agent status`
- [ ] Top-level `uninstall` with `--purge`, `--restore-audio`, `-y`
- [ ] State file `pre_install_default.json` on first switch
- [ ] Homebrew formula `uninstall` hook calling `rusty-jack uninstall -y`
- [ ] README: install / uninstall / troubleshooting
- [ ] **Tests:** `launchd.rs` — plist render golden file, install paths, `bootout` idempotency (mock `Launchctl` trait); uninstall orchestration with temp dirs

### Phase 6 — Hardening (1–2 days)

- [ ] Edge cases: device unplugged, Bluetooth, aggregates
- [ ] QA matrix: macOS 12 Intel, macOS 13+ ARM
- [ ] `scripts/build-universal.sh` + release smoke test
- [ ] **Tests:** fill gaps to meet coverage target; `tests/fixtures/` regression JSON; optional `assert_cmd` CLI tests

**Milestone after Phase 6:** Reliable **routing** to HDMI/DP — useful for testing and scripts, but **volume keys still broken** on typical external displays until Phase 7.

### Phase 7 — Virtual driver + software volume (core value) (2–4 weeks)

Delivers the eqMac-class fix for keyboard volume on HDMI/DP:

- [ ] **AudioServerPlugIn** virtual output device (study eqMac `native/driver`, [tympan-aspl](https://github.com/penta2himajin/tympan-aspl), Apple null-driver sample)
- [ ] `driver install` / `driver uninstall` (copy to `/Library/Audio/Plug-Ins/HAL/`, restart `coreaudiod` or document reboot)
- [ ] Daemon **passthrough loop**: read from virtual device, apply **software volume** (sync with volume keys / `kAudioDevicePropertyVolumeScalar`), write to configured physical UID
- [ ] Set virtual device as **default output** + **default system output** when driver is active
- [ ] `uninstall` removes driver and restores prior physical default
- [ ] **Tests:** ring-buffer / gain math unit tests; mock render path; driver property handlers where testable off-hardware

**Definition of done (Phase 7):** User selects HDMI/DP monitor; **F10/F11/F12 change audible volume**; `rusty-jack list` shows virtual + physical devices; clean uninstall restores pre-install audio stack.

### Phase 8 — Sony SRS-ZR5 wake on user input activity (1–2 weeks)

Wake an **SRS-ZR5** when Mac **line-out** is the target output and the user shows **presence at the Mac** (mouse or keyboard activity). **Native Rust ScalarWebAPI client** — no Python.

- [ ] Config block `sony_speaker` (§4.1) + validation
- [ ] **`src/sony/scalar_api.rs`** — HTTP POST envelope, `getSupportedApiInfo`, per-service `call(method, params)`, error types
- [ ] **`src/sony/power.rs`** — `getPowerStatus`, `setPowerStatus(status: active|off)`; skip wake if already active
- [ ] **`SpeakerWake` trait** + `ScalarWebSpeakerWake` + **`MockSpeakerWake`** for tests
- [ ] **`src/activity/macos.rs`** — `CGEventTap` for keyboard (`kCGEventKeyDown`) and mouse (move, click, scroll); `UserActivityMonitor` trait + mock
- [ ] Hook into **daemon** policy loop: on `output_selected` when default switches to line-out
- [ ] Debounce / status cache to limit LAN traffic
- [ ] **`rusty-jack sony discover`** (Phase 8.1) — optional SSDP/UPnP endpoint discovery in Rust (reference: python-songpal discover)
- [ ] **Tests:** wiremock/httptest fixtures for guide + system responses; wake only when UID + trigger match; debounce; no wake when HDMI is default; mock activity events without real event tap

**Definition of done (Phase 8):** Line-out configured as preferred; move mouse or press a key while ZR5 is in standby → Rust client POSTs `setPowerStatus` → speaker wakes; no Python installed; no wake when output is HDMI; Accessibility permission documented; Quick Start-Up documented.

**Future (Phase 8+):** WebSocket notifications (`notifyPowerStatus`); Wake-on-LAN from `getSystemInformation` MAC; input select on ZR5 if needed.

**Total estimate:** ~9–12 days for Phases 0–6; **+2–4 weeks** for Phase 7; **+1–2 weeks** for Phase 8 (Rust ScalarWebAPI + input activity monitor).

**Definition of done (every phase):** feature code + unit tests for touched modules + green `cargo test`.

---

## 14. Future extensions (beyond core volume + routing)

| Feature | Complexity | Approach |
|---------|------------|----------|
| **Sony SRS-ZR5 wake on user input** | **Medium** | Phase 8 — native Rust ScalarWebAPI (`reqwest`) + `CGEventTap` activity monitor; see §1.1 |
| EQ / “bass boost” | **Medium** | Extend passthrough pipeline (Phase 7) with DSP — eqMac uses `AVAudioEngine` for this |
| Per-app routing | **High** | Prism-style virtual bus or Background Music fork |
| Notarization / automated driver signing | **Medium** | Required for wide distribution of the virtual driver outside Homebrew |

For **HDMI/DP volume control (the core problem), Phase 7 is required** — see §0.

---

## 15. Testing strategy

### 15.1 Test pyramid

| Layer | Scope | Runs in CI |
|-------|--------|------------|
| **Unit** | Every component (table below) | yes (`cargo test`) |
| **Integration** | CLI via `assert_cmd`, fixture configs | yes (macOS runner) |
| **Manual / hardware** | Real HDMI, sleep/wake, dock | no (release checklist) |

### 15.2 Unit tests per component (required)

| Component | Test file | What to test |
|-----------|-----------|--------------|
| **`config`** | `config.rs` `mod tests` | Deserialize/serialize round-trip; defaults; invalid `version`; unknown fields denied or ignored per policy; `config_path()` resolution with env override |
| **`error`** | `error.rs` `mod tests` | Display messages; `From` conversions; error codes map to exit status |
| **`cli`** | `cli.rs` `mod tests` | Every subcommand parses; global flags; `--json`; invalid args fail with usage |
| **`policy`** | `policy.rs` `mod tests` | `desired_device()`: preferred connected/disconnected, fallback order, allowlist, transport filter; `should_switch()`: already on target, `auto_switch` off, debounce flag |
| **`daemon`** | `daemon.rs` `mod tests` | One poll cycle applies policy; debounce suppresses rapid switches; shutdown signal stops loop; wake burst schedules extra polls (mock scheduler) |
| **`launchd`** | `launchd.rs` `mod tests` | Plist XML matches golden snapshot; label derived from bundle id; install writes correct paths; uninstall removes plist when missing job; `agent status` parses launchctl output (fixture stdout) |
| **`coreaudio::device`** | `device.rs` `mod tests` | Map mock device list → `OutputDevice`; HDMI filter; exclude denylist names; `is_output_device` heuristic |
| **`coreaudio::default_output`** | `default_output.rs` `mod tests` | Get/set via `MockHal`; `also_set_system_output`; restore UID from state file |
| **`coreaudio::listener`** | `listener.rs` `mod tests` | Property address construction; listener register/unregister; events forwarded on mock HAL (no real `CFRunLoop`) |
| **`coreaudio::sys`** | `sys.rs` | Minimal: FourCC encode/decode helpers only — **no live HAL calls** in unit tests |
| **`main`** | — | Thin; coverage via `cli` + integration tests only |

### 15.3 Test doubles (shared)

```rust
// src/coreaudio/traits.rs (or src/traits.rs)

pub trait AudioHal: Send + Sync {
    fn output_devices(&self) -> Result<Vec<OutputDevice>>;
    fn default_output_uid(&self) -> Result<Option<String>>;
    fn set_default_output(&self, uid: &str) -> Result<()>;
}

pub struct MockHal { /* Vec<OutputDevice>, call log */ }
```

Use **`tests/fixtures/devices.json`** for policy/daemon table tests and **`tests/fixtures/launchagent.golden.plist`** for launchd.

Example policy table test:

```rust
#[test]
fn test_policy_prefers_connected_hdmi_over_builtin() {
    let cfg = fixture_config();
    let devices = fixture_devices_hdmi_and_builtin();
    let hal = MockHal::new(devices).with_default("builtin-uid");
    assert_eq!(policy::desired_device(&cfg, &hal).unwrap().uid, "hdmi-uid");
}
```

### 15.4 CI test job

```yaml
# .github/workflows/ci.yml (excerpt)
- run: cargo test --all-targets --all-features
- run: cargo llvm-cov --all-features --lcov --output-path lcov.info  # optional
```

Run on `macos-13` or `macos-14` (Linux runners skip macOS-only tests via `cfg`).

### 15.5 Manual / hardware matrix (not unit tests)

| Scenario | Method |
|----------|--------|
| **Volume keys on HDMI/DP** | Phase 7: F10/F11/F12 change audible level with virtual driver installed |
| **SRS-ZR5 wake on line-out** | Phase 8: line-out preferred + keyboard/mouse activity → Rust `setPowerStatus`; Quick Start-Up enabled |
| Built-in ↔ HDMI | `set`, `apply`, `daemon` |
| Sleep/wake + dock | daemon poll after wake |
| Uninstall | `uninstall`, `brew uninstall` — no orphan plist |
| Cross-arch | x86_64 on Intel 12.x; arm64 on M-series |

Record device UIDs from `list --json` into `tests/fixtures/` when adding regression cases.

---

## 16. Risks and mitigations

| Risk | Mitigation |
|------|------------|
| **Volume keys have no effect on HDMI/DP** | Phase 7: virtual device as default + software volume passthrough |
| CoreAudio doesn’t switch until run loop / delay | Use `switch_delay_ms`; call `CFRunLoopRunInMode` briefly after set |
| HDMI connected while asleep (eqMac #829) | Polling + post-wake burst enumeration |
| Bluetooth HFP devices can’t be default output | Exclude by transport; document limitation |
| Device UID changes after firmware update | Support `name_contains` + `list` to update config |
| User fights daemon with Sound settings | Optional manual grace period; log when overriding |
| `coreaudio-rs` API gaps for set-default | Thin `unsafe` module using `AudioObjectSetPropertyData` directly |
| Cross-compile link errors on CI | Pin deployment target; install both rustup targets; compile both in CI |
| Uninstall leaves audio on HDMI | Default `--restore-audio`; document `--no-restore-audio` for Brew |
| macOS 12 API drift | Test on darwin 21.x hardware; avoid macOS 15-only Swift APIs |
| **SRS-ZR5 stays asleep on line-out** | Phase 8: ScalarWebAPI wake when `mac_output_uid` active + keyboard/mouse trigger |
| Songpal wake fails (ZR5 asleep) | Enable **Quick Start-Up** on speaker; verify endpoint with `rusty-jack sony discover` or curl; debounce + log failures |
| Input activity tap denied | Document Accessibility permission; fallback to `output_selected` trigger only |

---

## 17. References

- eqMac README (driver + app split): https://github.com/bitgapp/eqMac/blob/master/README.md
- eqMac outputs filter: `native/app/Source/Audio/Outputs/Outputs.swift`
- eqMac passthrough / selection: `native/app/Source/Application.swift`
- eqMac virtual driver: `native/driver/Source/EQMDevice.swift`, `EQMInterface.swift`
- Apple AudioServerPlugIn: https://developer.apple.com/documentation/coreaudio/creating_an_audio_server_driver_plug-in
- `coreaudio-rs` macOS helpers: https://docs.rs/coreaudio-rs/latest/coreaudio/audio_unit/macos_helpers/
- Similar CLI (priority list): https://github.com/mateusbadalotti/audio-priority-cli-macos
- audioswitch (C reference for set/get default): https://github.com/retrography/audioswitch/blob/master/device.c
- **python-songpal** (protocol reference for ScalarWebAPI): https://github.com/rytilahti/python-songpal
- Sony Audio Control API (ScalarWebAPI) — see python-songpal `device.py` / `service.py` for request shapes
- Home Assistant Songpal integration (SRS-ZR5 listed): https://www.home-assistant.io/integrations/songpal/

---

## 18. Decision log (recommended defaults)

1. **The core problem is software volume on HDMI/DP** — routing-only tools cannot solve it; a virtual device is required.
2. **No virtual driver in Phases 1–6** — routing foundation ships first; **Phases 1–6 alone do not fix volume keys**.
3. **UID-based config** — not eqMac’s numeric `AudioDeviceID` (session-unstable).
4. **Listeners + poll** — don’t rely on listeners alone (eqMac wake bugs).
5. **LaunchAgent not LaunchDaemon** — per-user audio context; no root for the agent itself (but root **is** required to install a HAL plugin).
6. **`clippy -D warnings`** — keep codebase clean from day one.
7. **macOS 12.0 minimum** — supports older Intel Macs (e.g. Monterey 12.7); no macOS 11 in v1.
8. **Dual-target releases** — always ship `aarch64` + `x86_64` artifacts; optional universal via `lipo`.
9. **Clean uninstall is first-class** — `uninstall` subcommand + Homebrew hook; state file enables audio restore.
10. **Unit tests per component** — no module merges without colocated tests; CoreAudio behind `AudioHal` mock.
11. **Sony SRS-ZR5 wake is config-driven** — map line-out UID → ScalarWebAPI endpoint; wake on **keyboard/mouse activity** via **native Rust HTTP client** (`system.setPowerStatus`); python-songpal is reference documentation only, not a runtime dependency.

---

*Document version: 1.6 — Phase 8: wake SRS-ZR5 on mouse/keyboard activity (not volume keys).*
