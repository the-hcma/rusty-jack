# Code Review Findings

This document consolidates 20 code review issues identified across the repository.

## Summary Table

| # | Severity | Issue | Affected File(s) |
|---|---|---|---|
| 1 | High | Discarded `set_output_volume` result in `ensure_startup_volume` | `src/daemon.rs` |
| 2 | Code Quality | Duplicated ScalarWebAPI fallback helper functions | `src/daemon.rs` |
| 3 | High | Fragile/double `is_alive` fallback logic | `src/policy.rs` |
| 4 | High | Non-atomic config rewrite race | `src/config.rs` |
| 5 | High | `cfg!(test)` disabling persistence in library code | `src/volume_memory.rs`, `src/state.rs` |
| 6 | High | Inconsistent config reload error handling | `src/daemon.rs` |
| 7 | High | Blocking sleeps on daemon thread during eqMac startup/restart | `src/eqmac.rs`, `src/daemon.rs` |
| 8 | High | Reachability check ignoring `CONNECTION_REQUIRED` | `src/network.rs` |
| 9 | Medium | Expensive `ioreg` polling every second | `src/activity.rs` |
| 10 | High | TOCTOU race from `path.exists()` pre-checks | `src/config.rs`, `src/state.rs` |
| 11 | Medium | Possible drift between `volume_for_target` and passthrough equivalent | `src/apply.rs`, `src/passthrough.rs` |
| 12 | Code Quality | Too-many-arguments daemon helper | `src/daemon.rs` |
| 13 | Code Quality | Duplicated `DaemonHooks` construction | `src/daemon.rs` |
| 14 | Code Quality | No-op `OutputDevice::friendly_label` | `src/output_device.rs` |
| 15 | Code Quality | No-op `friendly_label` helper in `apply.rs` | `src/apply.rs` |
| 16 | Medium | Misuse of `RustyJackError::Launchd` for app launch failures | `src/eqmac.rs`, `src/error.rs` |
| 17 | Code Quality | Overly broad `allow(non_upper_case_globals)` | `src/transport.rs` |
| 18 | Medium | Config JSON parsed twice per load | `src/config.rs` |
| 19 | Code Quality | Missing `#[must_use]` on signal-returning methods | `src/daemon.rs`, `src/eqmac.rs` |
| 20 | Code Quality | Misleading unused `_monitor` test helper parameter | `src/daemon.rs`, `src/apply.rs` |

## High Priority — Correctness / Logic Bugs

### 1) Discarded `set_output_volume` result in `ensure_startup_volume`
- **Affected file(s):** `src/daemon.rs`
- **Explanation:** `ensure_startup_volume` calls `hal.set_output_volume(...)?` but discards the returned `VolumeEnsureResult` with `let _ = ...`.
- **Impact:** Startup volume application loses verification/read-back visibility, so incorrect startup volume can go unnoticed.
- **Suggested fix:** Capture and log/use the returned result, or explicitly document why verification data is intentionally ignored.

### 3) Fragile/double `is_alive` fallback logic
- **Affected file(s):** `src/policy.rs`
- **Explanation:** `alive_device()` plus an additional `if device.is_alive` check creates confusing fallback flow that is easy to break during future refactors.
- **Impact:** Preferred-target selection behavior can silently regress if helper semantics change.
- **Suggested fix:** Make the alive check explicit in one place (direct `find` for alive match) or clearly document intended fallback behavior.

### 4) Non-atomic config rewrite race
- **Affected file(s):** `src/config.rs`
- **Explanation:** Canonicalization rewrites config via `std::fs::write`, which truncates and rewrites in place.
- **Impact:** Concurrent reload/write paths can observe partial file contents, causing transient corruption or parse failures.
- **Suggested fix:** Use atomic rewrite (write temp file in same directory, then `rename` over target).

### 5) `cfg!(test)` disabling persistence in library code
- **Affected file(s):** `src/volume_memory.rs`, `src/state.rs`
- **Explanation:** Library logic disables persistence when compiled for tests by returning `None` paths under `cfg!(test)`.
- **Impact:** Tests miss real persistence behavior and production/test behavior diverges.
- **Suggested fix:** Inject storage path/abstraction and use temp directories in tests instead of changing production code behavior.

### 6) Inconsistent config reload error handling
- **Affected file(s):** `src/daemon.rs`
- **Explanation:** Event-driven reload paths warn-and-continue while scheduled reload path propagates errors and terminates.
- **Impact:** Same transient config error can either be tolerated or crash daemon depending on trigger source.
- **Suggested fix:** Unify behavior (prefer warn + keep last-known-good config in both paths).

### 7) Blocking sleeps on daemon thread during eqMac startup/restart
- **Affected file(s):** `src/eqmac.rs`, `src/daemon.rs`
- **Explanation:** `thread::sleep(EQMAC_STARTUP_WAIT)` runs on daemon main thread during startup/restart (up to multiple seconds).
- **Impact:** Event processing and polling are stalled; responsiveness and timing degrade.
- **Suggested fix:** Offload startup/restart to background thread/state machine, or at minimum document and tune blocking behavior.

### 8) Reachability check ignoring `CONNECTION_REQUIRED`
- **Affected file(s):** `src/network.rs`
- **Explanation:** Reachability currently checks only `REACHABLE`, not whether additional connection setup is required.
- **Impact:** Wake attempts may run when network is not actually ready, increasing timeouts/latency.
- **Suggested fix:** Require `REACHABLE` and reject `CONNECTION_REQUIRED` in readiness logic.

### 10) TOCTOU race from `path.exists()` pre-checks
- **Affected file(s):** `src/config.rs`, `src/state.rs`
- **Explanation:** Multiple call sites check `exists()` before read/write/remove operations.
- **Impact:** File state can change between check and use, producing racey and misleading failures.
- **Suggested fix:** Perform operation directly and handle `ErrorKind::NotFound` in error matching.

## Medium Priority — Reliability / Robustness

### 9) Expensive `ioreg` polling every second
- **Affected file(s):** `src/activity.rs`
- **Explanation:** Activity poll spawns `ioreg -c IOHIDSystem` each interval (default 1s).
- **Impact:** Repeated subprocess + registry traversal adds avoidable CPU overhead.
- **Suggested fix:** Replace with direct idle-time API (`CGEventSourceSecondsSinceLastEventType` or IOKit equivalent).

### 11) Possible drift between `volume_for_target` and passthrough equivalent
- **Affected file(s):** `src/apply.rs`, `src/passthrough.rs`
- **Explanation:** Two similar volume-selection helpers exist with overlapping purpose but different signatures.
- **Impact:** Logic divergence can produce inconsistent routing behavior over time.
- **Suggested fix:** Consolidate to one canonical helper or add equivalence tests for overlapping cases.

### 16) Misuse of `RustyJackError::Launchd` for app launch failures
- **Affected file(s):** `src/eqmac.rs`, `src/error.rs`
- **Explanation:** eqMac app-launch failures are wrapped in a launchd-specific error variant.
- **Impact:** Error classification/matching is misleading for callers and diagnostics.
- **Suggested fix:** Add/use dedicated app-launch error variant (or another semantically correct variant).

### 18) Config JSON parsed twice per load
- **Affected file(s):** `src/config.rs`
- **Explanation:** Config is parsed once into `Config` and again into `serde_json::Value` for canonicalization.
- **Impact:** Repeated parsing adds overhead on frequent reload paths.
- **Suggested fix:** Parse once into `Value`, canonicalize, then deserialize canonical JSON into `Config`.

## Code Quality / Maintainability

### 2) Duplicated ScalarWebAPI fallback helper functions
- **Affected file(s):** `src/daemon.rs`
- **Explanation:** Two helper functions with different names contain identical logic.
- **Impact:** Increases maintenance burden and risk of future divergence.
- **Suggested fix:** Merge into one helper or parameterize/document the semantic distinction.

### 12) Too-many-arguments daemon helper
- **Affected file(s):** `src/daemon.rs`
- **Explanation:** Helper takes 8 parameters and suppresses Clippy lint.
- **Impact:** Harder readability/testability and higher call-site churn risk.
- **Suggested fix:** Introduce a context struct for shared parameters.

### 13) Duplicated `DaemonHooks` construction
- **Affected file(s):** `src/daemon.rs`
- **Explanation:** `DaemonHooks` is built in two places with near-identical setup.
- **Impact:** New fields can be inconsistently wired during future edits.
- **Suggested fix:** Centralize construction and mutate only the required differing field.

### 14) No-op `OutputDevice::friendly_label`
- **Affected file(s):** `src/output_device.rs`
- **Explanation:** Method currently returns `self.name.clone()` without transformation.
- **Impact:** Adds indirection and implies formatting that does not exist.
- **Suggested fix:** Remove and inline current behavior, or implement/document actual formatting intent.

### 15) No-op `friendly_label` helper in `apply.rs`
- **Affected file(s):** `src/apply.rs`
- **Explanation:** Private helper simply calls `to_string()`.
- **Impact:** Extra indirection without value.
- **Suggested fix:** Inline conversion or implement intended labeling behavior.

### 17) Overly broad `allow(non_upper_case_globals)`
- **Affected file(s):** `src/transport.rs`
- **Explanation:** Module-level lint suppression is broader than necessary.
- **Impact:** Can hide unrelated naming issues in future additions.
- **Suggested fix:** Scope allow to specific macOS FFI imports/usages only.

### 19) Missing `#[must_use]` on signal-returning methods
- **Affected file(s):** `src/daemon.rs`, `src/eqmac.rs`
- **Explanation:** Several public methods return signal values (`bool`/state enums) that are easy to ignore accidentally.
- **Impact:** Silent loss of control-flow signals at call sites.
- **Suggested fix:** Add `#[must_use]` to signal-returning methods and optionally rename ambiguous boolean-returning APIs.

### 20) Misleading unused `_monitor` test helper parameter
- **Affected file(s):** `src/daemon.rs`, `src/apply.rs`
- **Explanation:** Test helper accepts monitor label parameter but ignores it and hardcodes name.
- **Impact:** Tests can misrepresent expected label behavior and confuse intent.
- **Suggested fix:** Use parameter as device name or remove parameter and update call sites.
