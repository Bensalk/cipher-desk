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
  the policy limit. Catalogue of 15 plain-language errors with advice; security
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
| `cargo test` (debug / release) | ✅ PASS — 38/38 each (8 input, 10 status, 5 bridge, plus Stage 2 tests) |
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
