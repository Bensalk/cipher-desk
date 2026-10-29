# Progress Record

Each stage records completed work, test results, known issues and the next step.
A test that could not be run is listed as **NOT RUN**, never as passed.

---

## Stage 1 — Research and Planning ✅ Approved (29 Oct 2026)

See [`plan/stage1-report.md`](plan/stage1-report.md) for the full report and
[`plan/suggestions.md`](plan/suggestions.md) for the approved suggestion list.

---

## Stage 2 — Project Foundation ⏳ Local checks complete; awaiting first CI run (29 Oct 2026)

### Objective
Repository, structure, toolchain, Rust core, Flutter app skeleton, bridge,
build configuration, automated tests, CI, and secret/privacy protection (SA-9).

### Completed
- **Repository hygiene (SA-9):** `.gitignore` excludes secrets, keystores, profiles,
  pins and logs; gitleaks rules (`.gitleaks.toml`) and a **pre-commit hook** block
  secrets, passwords on command lines and personal email addresses; commits use a
  GitHub *noreply* identity; licence (Apache-2.0), `NOTICE`, `SECURITY.md`,
  `CONTRIBUTING.md` (coding standards).
- **Rust workspace** (Rust 1.97.0 pinned): `unsafe` forbidden in the core, `unwrap`/
  `expect`/`println` denied, release profile strips symbols.
- **`rdpv-core`:**
  - `secret::SecretString`: never prints its value, wiped on drop, not cloneable.
  - `logging`: `sspi`/`picky` logs always off (SF-2); release builds capped at `info`
    (runtime clamp + `tracing/release_max_level_info` at compile time).
  - `policy::SecurityDefaults`: NLA required, TLS ≥ 1.2, certificate pinning
    (no "accept any" option exists), clipboard off, redirection off, 10 s connect
    timeout, 30 min idle disconnect, unlock before saved password, no retry after
    authentication failure.
  - `version`: build summary; links IronRDP 0.17 (crates.io).
- **`rdpv-bridge`:** flutter_rust_bridge 2.13.0 API (`hello_from_rust`,
  `core_summary`, `security_defaults_summary`) with generated bindings committed.
  Library name `rust_lib_secure_rdp_viewer`.
- **Flutter app (`app/`):** start screen calling the Rust core; integration test;
  Windows/Linux/Android platform folders taken from flutter_rust_bridge's own CI-tested
  2.13.0 example and renamed; Cargokit `rust_builder` pointed at `rust/bridge`.
- **Android security settings:** minSdk 29 (Android 10), `allowBackup=false` plus
  data-extraction rules excluding all app data from cloud backup and device transfer
  (S11), cleartext traffic off, only the INTERNET permission, `FLAG_SECURE` on by
  default (S10), release signing read from CI secrets or an untracked
  `key.properties` (S12).
- **CI (`.github/workflows`):** Rust fmt/clippy/tests (debug + release) and policy
  checks on Ubuntu + Windows; Flutter regenerate → analyze → test → build for Linux,
  Windows and Android, with integration tests on Linux (Xvfb) and Windows; security
  workflow with full-history gitleaks, cargo-deny, OSV-Scanner and CodeQL; Dependabot
  for Cargo, pub and Actions. Every action pinned to a commit hash.
- **Scripts:** `check-policy.sh` (fails on any "accept any certificate" code or enabled
  `sspi` logging), `release-build.sh` (removes local folder paths from binaries),
  `install-hooks.sh`.

### Test results (local, Ubuntu 24.04, Rust 1.97.0)

| Check | Result |
|-------|--------|
| `cargo fmt --check` | ✅ PASS |
| `cargo clippy --all-targets -D warnings` | ✅ PASS (0 warnings) |
| `cargo test` (debug) | ✅ PASS — 17/17 |
| `cargo test --release` | ✅ PASS — 17/17 |
| `scripts/check-policy.sh` | ✅ PASS |
| `cargo deny check` | ✅ PASS (advisories, bans, licences, sources), with one reviewed exception (KI-2) |
| Bridge native library builds and exports bridge functions | ✅ PASS (`librust_lib_secure_rdp_viewer.so`) |
| Release binary contains no local folder paths | ✅ PASS — 0 found (unstripped debug build: 1,479, so the check is effective) |
| gitleaks over full git history | ✅ PASS — no leaks |
| Pre-commit hook blocks a staged secret | ✅ PASS — test file with a password was rejected, nothing committed |
| Personal data in tracked files (email, name, local paths) | ✅ PASS — none (only gitleaks rule keywords) |
| Commit author/committer email | ✅ GitHub noreply address only |
| actionlint on workflows | ✅ PASS |
| Flutter analyze / test / build (Linux, Windows, Android) | ⛔ **NOT RUN** — Flutter SDK can't be downloaded in the build sandbox. Runs in CI. |
| Integration test Rust ↔ Flutter | ⛔ **NOT RUN** — needs Flutter; runs in CI on Linux and Windows. |
| Windows build of Rust core | ⛔ **NOT RUN** locally — runs in CI (`windows-latest`). |

### Known issues
- **KI-1 (SF-2, mitigated):** IronRDP's CredSSP library (`sspi`) logs credential
  buffers at debug/trace. Mitigated by always disabling `sspi` logs and capping
  release logging at info. A private report to Devolutions is still to be sent.
- **KI-2 (accepted risk, review later):** `RUSTSEC-2023-0071` (rsa "Marvin" timing
  attack, no upstream fix) reaches us through IronRDP → sspi/picky. It affects RSA
  *private-key* operations. This client authenticates with passwords and performs only
  public-key operations; `sspi` 0.21.3 has no private-key use outside tests. Recorded
  as an exception in `deny.toml` and `osv-scanner.toml`. Re-review if smart-card login
  is added or a patched `rsa` is released.
- **KI-3:** crates.io IronRDP 0.17.0 uses `sspi` 0.21.3, whereas the Stage 1 spike
  (IronRDP git master) used `sspi` 0.23.1. Stage 4 re-runs the spike tests (T1–T7)
  against the crates.io version.
- **KI-4:** Dart bindings were generated without Dart formatting; CI regenerates and
  formats them. The Flutter/Android/Windows parts are unverified until the first CI run.
- **KI-5:** `allo-isolate` (a flutter_rust_bridge dependency) declares its licence by
  file only (Apache-2.0); cargo-deny reports a warning, not a failure.

### Next step
Push to a **private** GitHub repository and let CI run. Stage 2 is complete
only when CI is green.

---

## Stage 3 — User Interface ⏳ Rust logic tested; Flutter screens awaiting first CI run (29 Oct 2026)

Started before Stage 2's CI run at the user's request ("continue"), because the
GitHub repository does not exist yet. Any CI failures in Stage 2 or 3 are fixed
before Stage 4 begins.

### Objective
Main window, connection form (host/IP, port, username, masked password, Save
Password checkbox), saved connections, status indicator, settings, clear error
messages; layouts for desktop and phone.

### Completed
- **Rust `input`:** validates host names (RFC 1123), IPv4, IPv6 (`[v6]:port`),
  ports 1–65535 (also `host:port` in the host field), usernames (`user`,
  `DOMAIN\user`, `user@domain`), password length/control characters and profile
  names. All errors are collected per field. Messages never repeat the input.
- **Rust `status`:** connection state machine (connecting, check identity,
  connected, reconnecting, disconnecting, failed). Wrong password, account problems
  and certificate changes **never auto-retry** (SA-8); network drops reconnect up to
  the policy limit. Catalogue of 16 plain-language errors with advice (16th, "Connection cancelled", added in the review); security
  warnings flagged.
- **Bridge:** `check_connection_form`, `default_port`, `all_connection_errors`.
  Bindings regenerated with flutter_rust_bridge 2.13.0.
- **Flutter:**
  - Home: searchable connection list (F1), recently used first, edit/delete with
    confirmation, empty state, status chip, error banner. Side rail on wide screens,
    bottom bar on phones.
  - Connection form: masked password with show/hide, no autocorrect, suggestions or
    keyboard learning on the password field; Save Password checkbox with vault
    explanation; per-connection clipboard switch (off by default) with a warning;
    validation by the Rust core.
  - Settings: theme (system/light/dark), Android "Block screenshots" (on by default,
    S10), clipboard default for new connections (off), security defaults, error help,
    version and open-source licences.
  - Profiles hold **no password field**; passwords go only to the OS vault (Stage 5).
  - Connect shows a clear "not available yet" notice until Stage 4.
- **Android:** `MainActivity` channel `rdpv/secure_window` to turn `FLAG_SECURE` on/off.

### Test results (local, Rust 1.97.0)

| Check | Result |
|-------|--------|
| `cargo fmt --check` | ✅ PASS |
| `cargo clippy --all-targets -D warnings` | ✅ PASS |
| `cargo test` (debug / release) | ✅ PASS — 40/40 each after the review (8 input, 12 status, 5 bridge, 15 Stage 2) |
| Policy, cargo-deny, gitleaks pre-commit | ✅ PASS |
| `unsafe` in hand-written code | ✅ 0 |
| Dart unit tests (`test/profile_store_test.dart`, 6 tests) | ⛔ **NOT RUN** — no Flutter SDK here; runs in CI |
| Integration tests (5 UI tests against the real Rust core) | ⛔ **NOT RUN** — runs in CI on Linux and Windows |
| `flutter analyze`, builds for Windows/Linux/Android | ⛔ **NOT RUN** — runs in CI |
| Android screenshot toggle on a device | ⛔ **NOT RUN** — needs an Android device or emulator |

### Known issues
- **KI-6:** The Dart/Kotlin code has never been compiled. Expect small fixes after
  the first CI run.
- **KI-7:** Settings and profiles are in memory only; saving to disk is Stage 7.

### Next step
Create the private GitHub repository so CI can verify Stages 2–3, then Stage 4
(connection engine).

---

## Review of Stages 1–3 (29 Oct 2026)

A full review of the plan, documents, Rust, Dart, Kotlin, CI and repository before
starting Stage 4. Corrections made:

| # | Area | Problem found | Fix |
|---|------|---------------|-----|
| R1 | Repository | A deliverable zip (`secure-rdp-viewer-stage3.zip`) had been committed into the repository | Removed from the (unpushed) Stage 3 commit, so it is not in the history; `*.zip`, `*.bundle`, `*.tar.gz`, `*.7z` added to `.gitignore` |
| R2 | Plan | No stage covered keyboard, mouse and touch input | Added to Stage 4 (with Android touch/trackpad modes and special-keys bar, F8–F10) |
| R3 | Plan | Stack table said flutter_rust_bridge 2.12 and "IronRDP 0.17 commit f01f9b5" | Now 2.13.0 (pinned), and the prototype commit is separated from the published crates the app uses |
| R4 | Plan | "Approval needed" section still asked for Stage 1 approval | Replaced with the approval record and the two items still open |
| R5 | Docs | Suggestions file still said "awaiting approval" and could be mistaken for the plan | Marked approved, with a note that the Stage 1 report takes precedence |
| R6 | Docs | README status still said Stage 2 | Updated |
| R7 | Rust | An error arriving while disconnecting was an invalid status change | Now finishes the disconnect; test added |
| R8 | Rust | Rejecting an unknown certificate showed the "compare the fingerprint" message | New error "Connection cancelled — nothing was sent"; test added |
| R9 | Rust | A bad port was not reported while the host field was also invalid | Both are now reported together; tests updated |
| R10 | Dart | "Save password" text promised fingerprint/face/PIN, which is wrong for Linux | Text now covers device password too |
| R11 | Dart | Status chip had no "Disconnecting" state although the core has one | Added |
| R12 | Tests | Integration tests tapped form fields without scrolling them into view, and looked for an entry in a lazily built list | Tests now scroll before tapping and scroll the error list; also check the new port message |
| R13 | Security tooling | The command-line password rule also matched `cargo -p <crate>` | Rule now matches `--password` and `/p:` only; verified to catch both and ignore `cargo -p` |

Checked and found correct: generated bindings in sync with the Rust API (regenerated,
no differences); Dart code matches the generated signatures; Kotlin channel matches
the Dart channel name and method; Android manifest/Gradle security settings; CI
workflows (actionlint); dependency, licence and secret checks; no personal data in
tracked files or commit metadata; 0 `unsafe` in hand-written code.

Still unverified (needs the GitHub repository): compiling and running the Flutter,
Kotlin and Windows parts.

---

## Stage 4 — Connection Engine ✅ Engine verified against live hosts; Flutter session screen awaiting first CI run (29 Oct 2026)

### Objective
Secure connection (TLS + NLA) with certificate pinning, timeouts, remote screen,
keyboard/mouse/touch input, server reactivation and graceful disconnect.

### Completed
- **Dependencies:** IronRDP sub-crates used directly at their latest matching releases
  (connector 0.12, session 0.13, graphics 0.11, pdu 0.11, blocking 0.12, input 0.9).
  The `ironrdp` 0.17 umbrella crate pinned older sub-crates; this also resolves KI-3.
- **`pins`:** trust-on-first-use certificate pins (SHA-256), per host:port, saved
  atomically with owner-only permissions; a damaged or tampered file is an error, never a
  silent reset.
- **`tls`:** rustls with an explicit crypto provider, TLS 1.2/1.3 only, accepts only the
  pinned certificate and still verifies handshake signatures. No "accept any" mode exists.
- **`session`:** TCP (10 s timeout) → pinned TLS → NLA (CredSSP) → capability exchange;
  screen updates; keyboard (scancodes and Unicode), mouse and wheel input; server
  Deactivation-Reactivation (SF-4); graceful disconnect that releases held keys; idle
  disconnect (S7); errors mapped to the fixed user messages. A fixed client name is sent,
  so the device's own name is not revealed to the host.
- **Bridge:** `RdpSession.create()` + `start(...)` stream (status, certificate prompt,
  certificate changed, resize, frame, ended) and input/approval/disconnect methods.
- **Flutter:** password prompt (saved passwords arrive in Stage 5), certificate approval
  dialog showing the fingerprint, certificate-changed warning with both fingerprints,
  remote screen, mouse/keyboard (physical keys → PC scancodes), touch (tap, long-press for
  right-click, drag), special-keys bar (Esc, Tab, Win, Ctrl+Alt+Del, arrows, F1–F12) and
  "Type text" for on-screen keyboards. Pins stored in the app's private data folder.
- **CI:** new `live-host` job starts a FreeRDP shadow server (NLA) and xrdp (no NLA) with a
  random password per run (`scripts/start-test-hosts.sh`) and runs the live tests in debug
  and release.

### Test results (Ubuntu 24.04, Rust 1.97.0)

| Check | Result |
|-------|--------|
| `cargo fmt --check`, `cargo clippy --all-targets -D warnings` | ✅ PASS |
| Unit tests (debug) | ✅ PASS — 56/56 |
| Live tests vs local hosts — debug, 3 consecutive runs | ✅ PASS — 9/9 each |
| Live tests — release | ✅ PASS — 9/9 |
| T1 unknown certificate never trusted automatically | ✅ |
| T2/T3 approved certificate connects; screen received (non-blank) | ✅ |
| T4 changed certificate blocked before credentials are sent | ✅ |
| T5 wrong password → "Wrong username or password" | ✅ |
| T6 closed port → "refused"; unroutable address → no hang | ✅ |
| T7 password never in logs (plain or UTF-16 hex) at the most detailed allowed level; control run proves the check detects the sspi leak when unfiltered | ✅ |
| T8 host without NLA (xrdp) refused → "Secure connection not supported" | ✅ |
| T9 mouse, wheel, keyboard and Unicode input; graceful disconnect | ✅ |
| T10 requested size differs from the host's → session continues (SF-4) | ✅ |
| NLA against a **Windows** host | ⛔ **NOT RUN** — no Windows host available here |
| Flutter session screen, input mapping, Dart tests (`scancodes_test.dart`) | ⛔ **NOT RUN** — needs Flutter (CI) |
| Android/Windows builds with the engine | ⛔ **NOT RUN** — needs CI |

### Problems found and fixed during the stage
- `poll` kept reading while the screen changed continuously and never returned (hang found
  by the live tests). Fixed with a 50 ms processing budget per poll; 3 clean full runs since.
- xrdp's refusal of NLA was reported as "Something went wrong"; now "Secure connection not
  supported" (test uses the exact server text).
- Username format (`DOMAIN\user` vs `user@domain`) was lost after validation; now kept so
  each is sent correctly.
- flutter_rust_bridge turned `start_session` into a plain stream and dropped the handle;
  split into `create()` + `start()`.
- The secret-scan rule flagged `"$PASSWORD"` (a variable reference); it now ignores
  variables and still catches real literals (tested both ways).

### Known issues
- **KI-8:** Windows-host and Kerberos (domain) sign-in are untested.
- **KI-9:** Hosts that do not accept fast-path input (rare, e.g. some VirtualBox VRDP
  versions) are not supported yet; input uses fast-path only.
- **KI-10:** Server redirection (RD Connection Broker) is not followed; reported as an
  error.
- **KI-11:** Changed certificates cannot be re-trusted from the app yet; that needs the
  pin management screen (Stage 7). Until then a reinstalled host requires removing the
  pin file.
- **KI-12:** Screen updates are copied in full rectangles over the bridge; fine on a LAN,
  to be optimised if slow on phones (Stage 8).

### Next step
Stage 5 — secure credential management (OS vaults, biometric/PIN unlock, delete).
CI still needs the private GitHub repository.

