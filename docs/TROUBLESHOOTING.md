# Troubleshooting

Common issues when using Rusty Jack on macOS with HDMI/DP monitors, HDMI/DisplayPort volume control, launchd, and ScalarWebAPI device wake support.

## Volume keys do nothing on the monitor

**Cause:** The system default is a **physical HDMI/DP device** with no software volume scalar.

**Fix (today):**

1. Run `rusty-jack status` and check the **HDMI/DisplayPort Volume Control** block.
2. If **[eqMac](https://eqmac.app) is already installed**, let Rusty Jack use it (`apply`, `picker`, or daemon). That is the supported HDMI/DP volume-key path until release builds ship a **Developer ID–signed** `RustyJack.driver`.
3. Run `rusty-jack apply` or pick your monitor in `picker`.
4. Confirm a virtual volume-control device is the **system default** in Sound settings (or `rusty-jack status` shows a virtual default footer routing to your monitor).

**Native driver:** Homebrew and release packages bundle `RustyJack.driver`, but it is **ad-hoc signed**. macOS usually **refuses to load** it (Console: `signature not valid: -67050`). **Release builds do not prompt to install it** during `install`, `picker`, or `upgrade` until a signed driver ships — see [DRIVER_SIGNING.md](./DRIVER_SIGNING.md). Developers can run `make sign-driver-bundle` with a Developer ID certificate, use `rusty-jack driver swap-in`, or set `RUSTY_JACK_OFFER_NATIVE_DRIVER=1` to test install prompts locally.

---

## eqMac is installed but volume still wrong

1. **Quit and restart eqMac** — or let rusty-jack launch it via `apply` / HDMI `picker`.
2. Wait a few seconds after switch — config `volume` uses retries for eqMac reset races.
3. Check `rusty-jack status` — `volume` and `config volume` should align after a successful apply.
4. In eqMac UI, confirm the **physical output** matches your monitor (not built-in).

---

## eqMac is running but volume keys still do nothing

**Cause:** eqMac’s app process is alive, but CoreAudio’s system default is still the **physical** HDMI/DisplayPort device (no `EQMOutputCapture` / `… (eqMac)` virtual default). Common after **login, screen unlock, or sleep/wake** when the LaunchAgent was already running.

**Fix:**

1. Confirm with `rusty-jack status`: preferred HDMI/DP is active, eqMac is installed, and the **System default (virtual)** footer is **missing**.
2. The daemon should auto-recover: on **idle→active** (unlock/activity), on **scheduled** polls while active, and on **`apply` / `picker`**, it restarts eqMac when the process is up but the virtual default is missing (60s cooldown).
3. After recovery, status should show `System default (virtual)` routing to your monitor, and `osascript -e 'get volume settings'` should report a numeric `output volume`.
4. Manual workaround: quit and relaunch eqMac, then re-check status.

---

## `apply` says switched but I hear nothing

- Wrong device in config — run `list`, fix `preferred_device.uid`; `preferred_device.name` is only the human-readable label.
- HDMI/DisplayPort volume-control target mismatch — if using installed eqMac fallback, set output inside eqMac to your HDMI device.
- Monitor input/source — select correct HDMI input on the display.

---

## `apply` / `status`: policy “no change”

Default output already matches preferred UID. Expected when already routed correctly.

To force re-apply volume, switch away and back, or temporarily change preferred in config.

---

## Picker: ZoomAudioDevice (or similar) does nothing

**Expected.** Zoom and other **app virtual** drivers are **not speaker outputs**. They appear **dimmed** and cannot be selected. Pick your HDMI, DisplayPort, or Built-in row instead.

---

## `picker` requires a TTY

Use `--index N` from `list` IDX column for scripts:

```bash
rusty-jack list
rusty-jack picker --index 2 --json
```

---

## HDMI/DisplayPort volume-control note

`rusty-jack status` and `install` may report that HDMI/DisplayPort volume keys need a virtual volume layer and that **native driver install is not offered** until a signed release ships.

Use **eqMac** if already installed. Routing to HDMI still works; **volume control** needs eqMac or a future signed native driver.

---

## Native driver installed but no “Rusty Jack” output in Sound settings

**Expected on release/Homebrew installs.** `rusty-jack status` can report the driver bundle while CoreAudio has not published the virtual output because the shipped bundle is **not Developer ID–signed**.

1. Install the HAL bundle under **`/Library/Audio/Plug-Ins/HAL/RustyJack.driver`** (not only `~/Library/...`). `rusty-jack driver swap-in` does this with sudo.
2. Restart CoreAudio: `sudo killall -9 coreaudiod` (wait a few seconds).
3. Confirm the virtual device: `rusty-jack list` should include **Rusty Jack** with UID `com.the-hcma.rusty-jack.driver.output`.
4. Rebuild from source (`make install` or `make driver-bundle`) so the bundle is ad-hoc signed and uses the shared ring at `/Library/Application Support/rusty-jack/passthrough.ring`.
5. Production Macs may require a **Developer ID–signed** driver; ad-hoc signatures are often rejected by AMFI (`signature not valid: -67050` in Console). See [DRIVER_SIGNING.md](./DRIVER_SIGNING.md) and `make sign-driver-bundle`.

To restore eqMac after testing (whether or not a backup exists):

```bash
rusty-jack driver restore-eqmac
```

This removes `RustyJack.driver`, restores from the managed backup or reinstalls from `eqMac.app`'s embedded driver, and restarts `coreaudiod`. `rusty-jack driver swap-out` does the same when eqMac is installed.

### Fast HAL smoke test (agents / local iteration)

From a source checkout with the driver bundle built:

```bash
make driver-bundle
RUSTY_JACK_DRIVER_BUNDLE="$PWD/target/share/rusty-jack/RustyJack.driver" \
  RUSTY_JACK_HAL_DRIVER_SMOKE=1 \
  cargo test --test native_driver_hal_smoke -- --ignored --nocapture
```

This test quits eqMac, moves its HAL driver aside, installs Rusty Jack under `/Library/Audio/Plug-Ins/HAL/`, polls for the virtual output, opens the shared passthrough ring, and optionally starts the passthrough CoreAudio IO proc for a couple of seconds. Teardown always runs `restore-eqmac` logic when eqMac is installed (pass or fail). **Requires sudo** (password prompt) and an HDMI/DisplayPort monitor connected.

Run only the install/ring check:

```bash
RUSTY_JACK_HAL_DRIVER_SMOKE=1 cargo test --test native_driver_hal_smoke native_driver_hal_smoke_install -- --ignored --nocapture
```

Run only the passthrough engine check (after the above passes):

```bash
RUSTY_JACK_HAL_DRIVER_SMOKE=1 cargo test --test native_driver_hal_smoke native_driver_hal_smoke_passthrough_engine -- --ignored --nocapture
```

---

## Built-in speakers work; HDMI does not

Normal. Built-in has CoreAudio volume control. HDMI/DisplayPort needs the Rusty Jack native driver, or installed eqMac as fallback.

---

## Daemon is not switching outputs

1. Run `rusty-jack status` and confirm the daemon block says `running`, and the config path and preferred device match what you expect.
2. Check `auto_switch`; if it is `false`, the daemon stays alive but does not enforce policy.
3. Confirm the LaunchAgent is running:

```bash
launchctl print "gui/$(id -u)/com.example.rusty-jack"
```

4. Check logs:

```bash
tail -n 100 "$HOME/Library/Logs/rusty-jack.log"
```

`rusty-jack status` shows the log path in the Daemon block and the latest activity poll in the Activity block (idle time, console user, last idle→active transition, and last wake event when `event_tap` is active). Set `RUSTY_JACK_LOG_LEVEL=debug` before starting the daemon to log every activity poll; transitions log at `info` by default.

If the daemon wakes your ScalarWebAPI speaker when you are away, check for overnight `[activity] idle→active transition` lines without real use. Set `activity_monitor` to `event_tap`, keep `activity_event_tap_include_mouse_move` at `false`, and leave `activity_active_confirm_ms` at `5000` or higher. Grant Accessibility permission to `rusty-jack` when using `event_tap`. **Rusty Jack does not log keystrokes** — the tap only detects that input occurred (timing and coarse labels like `KeyDown`, not typed text). **Restart the daemon** after granting permission (`launchctl kickstart -k "gui/$(id -u)/com.example.rusty-jack"`). If permission was missing at startup, the daemon falls back to the idle monitor automatically once it detects a silent tap. Bluetooth mice that micro-move can still count as activity when `activity_event_tap_include_mouse_move` is `true`.

---

## Install did not propose my ScalarWebAPI speaker

1. Run interactive `rusty-jack install` (or `rusty-jack config init`) while the speaker is powered on, on the same LAN as the Mac, and has its network standby/wake option enabled.
2. Confirm the Mac has a working default route and LAN address (`rusty-jack status` activity/network context or `route -n get default`).
3. If discovery finds nothing, accept manual ScalarWebAPI setup and enter the speaker host (IP or hostname) yourself.
4. TV-class ScalarWebAPI devices on the network are intentionally skipped during install discovery. Rusty Jack targets network speakers, not TVs.

---

## ScalarWebAPI device does not wake

1. Confirm the selected Mac output matches `scalar_webapi_device.mac_output`.
2. Confirm `scalar_webapi_device.triggers` includes `keyboard` and `mouse` if you expect wake on screen unlock (re-run interactive `install` to upgrade a partial trigger list).
3. Confirm the device is reachable by hostname/IP and has its network standby/wake option enabled.
4. Set `scalar_webapi_device.port` to the device’s advertised ScalarWebAPI port (often `54480`). Wake falls back to this port (and any on-disk discovery cache) when SSDP misses; leaving the legacy default `10000` often fails.
5. Run `rusty-jack picker` and look for the ScalarWebAPI power-state note on the configured output.
6. Check daemon logs for wake errors or discovery warnings (`SSDP found no JSON-RPC endpoint … using configured` / `using cached`). `No route to host` right after unlock usually means Wi-Fi was not ready yet; the daemon defers wake until macOS reports the host reachable and retries after network changes.
7. If `rusty-jack scalar-webapi-device discover` finds 0 speakers while a manual SSDP probe on the same Mac succeeds, grant **Local Network** permission to `rusty-jack` (System Settings → Privacy & Security) and restart the daemon. Multicast discovery can fail even when unicast HTTP to the speaker works. Interactive `install` / `upgrade` re-probe SSDP when ScalarWebAPI is enabled and can open Local Network settings; they also force-restart the daemon so a new grant applies.

### Event tap permission and silent tap fallback

When `activity_monitor` is `event_tap` (set automatically when ScalarWebAPI wake triggers include `keyboard` or `mouse`):

**Privacy:** Rusty Jack is **not a keylogger**. The listen-only event tap does **not** record, log, or store what you type. It does not read key codes, characters, or passwords. macOS may phrase the permission as “receive keystrokes from any application”; Rusty Jack only uses it to detect that keyboard or pointer activity happened (for example “a key was pressed” or “the mouse moved”), so it can wake your speaker when you return to the Mac.

1. Grant **Accessibility** permission to `rusty-jack` in System Settings → Privacy & Security.
2. **Restart the daemon** after granting permission: `launchctl kickstart -k "gui/$(id -u)/com.example.rusty-jack"`, or run interactive `rusty-jack upgrade` / `upgrade --force` (install/upgrade re-check Accessibility when `activity_monitor` is `event_tap` and force-restart so the grant applies). Permission granted while the daemon is already running does not revive a disabled tap.
3. Run `rusty-jack status` and check the Activity block. While you use the Mac, `idle` should stay low (seconds, not hours). `state: idle` with a very large `idle` value while you are at the keyboard means the tap is silent — grant permission and restart, or set `"activity_monitor": "idle"` in config.
4. Look for `[activity] event tap disabled by macOS`, `[activity] event tap using idle monitor fallback`, or `[activity] event tap appears silent` / `event tap recreated after silent stall` in `~/Library/Logs/rusty-jack.log`. With `activity_event_tap_include_mouse_move: false` (the default), a silent tap is **recreated** automatically (about every 10 minutes at most) instead of falling back to platform idle, so Bluetooth pointer jitter does not cause speaker wakes. Fallback to the idle monitor only happens when `activity_event_tap_include_mouse_move` is `true`.

On daemon startup or after `upgrade`, a wake may still occur via the `output_selected` trigger even when activity detection is broken — that is separate from idle→active keyboard/mouse wakes.

Rusty Jack prefers the SSDP-advertised ScalarWebAPI port, then falls back to a discovery cache entry or config `port`/`path`.

`rusty-jack status` does not run LAN discovery; it only uses cached/configured endpoint details. Refresh cache with `rusty-jack list --discover` before troubleshooting stale host/model metadata.

### Verify ScalarWebAPI on your LAN (UPnP + JSON-RPC)

ScalarWebAPI “documentation” is served by the device itself. The reliable way to find the correct endpoints is UPnP/SSDP discovery:

- The device advertises a UPnP `LOCATION` URL (device description XML).
- That XML includes `X_ScalarWebAPI_BaseURL` (the JSON-RPC base).

Known-good examples from a discovered `SRS-ZR5` on a local network:

- Device description XML (UPnP `LOCATION`):
  - `http://192.168.86.18:54380/MediaRenderer_SRS-ZR5.xml`
- ScalarWebAPI SCPD (action list URL referenced by the UPnP service block):
  - `http://192.168.86.18:54380/ScalarWebApiSCPD.xml`
- ScalarWebAPI JSON-RPC endpoint (from `X_ScalarWebAPI_BaseURL`):
  - `http://192.168.86.18:54480/sony/system`

Important: the ScalarWebAPI service endpoints generally do **not** respond meaningfully to a browser GET. Use JSON-RPC POST:

```bash
curl -sS -X POST "http://192.168.86.18:54480/sony/system" \
  -H "Content-Type: application/json" \
  --data '{"method":"getPowerStatus","params":[],"id":1,"version":"1.1"}'
```

---

## Audio briefly falls back from ScalarWebAPI to internal speakers

The daemon only treats ScalarWebAPI failures as a reason to use fallback output when the Mac's network access fingerprint changed: active default interface, default gateway, or interface IP address. If that fingerprint is stable, the daemon keeps the ScalarWebAPI-backed Mac output selected and treats the failed ScalarWebAPI wake/status request as transient.

Check daemon logs for `selected ScalarWebAPI device is unreachable`. If those messages appear without a matching Wi-Fi/Ethernet or IP change, update Rusty Jack before tuning fallback configuration.

---

## `disable` vs `pause`

| Command | Use when |
|---------|----------|
| `disable` / `uninstall` | Removing rusty-jack from launchd entirely |
| `pause` | Temporarily stop auto-routing; you will `resume` later |

Neither stops external volume-control apps such as eqMac — manage them separately in System Settings or their own apps.

---

## Daemon still runs the old version after update

Homebrew and `cargo install` can both place a `rusty-jack` binary on your PATH. Upgrading one does **not** refresh the LaunchAgent — the daemon may keep running an older path until you run `rusty-jack upgrade --force`.

`rusty-jack status` reports `daemon stale: yes` when the running daemon’s version/commit differs from the CLI you invoked.

After `brew upgrade the-hcma/tap/rusty-jack`, run:

```bash
rusty-jack upgrade --force
```

Maintainers: `make do-release` offers step 6 to remove a stale `~/.cargo/bin/rusty-jack` (when Homebrew is installed), brew-upgrade, and refresh the daemon after a successful publish.

For source installs:

```bash
rusty-jack pause
git pull
make install
rusty-jack upgrade --force
```

If `rusty-jack --help` shows the new version but the daemon logs do not, the plist may point at an old binary path. Run `rusty-jack upgrade --force` to regenerate `~/Library/LaunchAgents/com.example.rusty-jack.plist` and restart the daemon.

---

## Confirm which binary you are running

```bash
./target/release/rusty-jack --help
# rusty-jack 0.1.0 (commit 7855685)

git rev-parse --short HEAD
# should match commit in --help
```

---

## CI / development

```bash
make fmt     # must pass before push
make clippy
make test
```

Hardware-specific tests are `#[ignore]`; HAL driver tests skip when matching drivers are not installed.

---

## Getting help

1. `rusty-jack status --json` — attach output (redact UIDs if needed).
2. `rusty-jack list --hdmi` — show connected HDMI/DisplayPort device names and UIDs.
3. Note macOS version, HDMI/DisplayPort Volume Control status, and eqMac version if eqMac fallback is installed.

File issues: [github.com/the-hcma/rusty-jack](https://github.com/the-hcma/rusty-jack/issues).

---

Copyright (c) 2026 Henrique Andrade / thehcma.
