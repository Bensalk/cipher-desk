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
- **KI-13 (end-of-stage review):** Our code wipes its own password copy (`SecretString`),
  but IronRDP's connector takes the password as a plain `String`, and Dart strings cannot
  be wiped; those copies live until freed. Inherent to the libraries; documented in
  SECURITY.md limitations at release.
- **KI-14 (end-of-stage review):** Host-name lookup uses the operating system resolver
  without its own timeout (the TCP connect and every read/write do have timeouts). A
  broken DNS server can delay the "Computer not found" message. Revisit in Stage 8.

### Next step
Stage 5 — secure credential management (OS vaults, biometric/PIN unlock, delete).
CI still needs the private GitHub repository.

---

## Stage 5 — Secure Credential Management ✅ Linux vault verified live; Windows and Android awaiting first CI run (29 Oct 2026)

### Objective
Optional saved passwords in the operating system's vault only; confirmation before use
(S5); deletion; no plain-text copies; refuse to save when no vault exists (I8).

### Completed
- **Rust `vault`** (`keyring-core` 1.0 with the Windows Credential Manager store and the
  Linux Secret Service store, pure-Rust D-Bus): save / load / exists / delete. Entries are
  named `connection-<random id>` under `io.github.securerdpviewer`, so the vault list
  reveals no host or user names. No vault → `Unavailable`, never a file fallback. Ids are
  validated before the store is touched.
- **Connecting with a saved password (Windows, Linux):** `RdpSession.startWithSavedPassword`
  reads the vault inside Rust, so the password never enters Dart.
- **Android vault (Kotlin):** AES-256-GCM key in the Android Keystore that can only be used
  for 30 s after fingerprint, face or screen-lock confirmation; ciphertext in private app
  storage excluded from backup; no screen lock → nothing saved; key invalidated by new
  biometric enrolment → all saved passwords deleted. `MainActivity` is now a
  `FlutterFragmentActivity` (needed for the system prompt).
- **Confirmation before use (S5):** Windows Hello via `local_auth` (if Windows Hello is not
  set up, the saved password is not used and the user types it); Android Keystore prompt;
  Linux keyring lock.
- **Flutter:** the form saves to / deletes from the vault and reports clearly when nothing
  was saved; connect uses the saved password after confirmation or asks for it; deleting a
  connection also deletes its saved password and trusted certificate.
- **Fix found during the stage (security):** connection ids were `p1`, `p2`, … and restarted
  each launch (profiles are only saved to disk in Stage 7), so a new connection could have
  picked up an old connection's saved password and sent it to a different computer. Ids are
  now random 128-bit values (Dart test added).
- **Fix found during the stage (tooling):** `start-test-hosts.sh` used a bash-only feature
  under `sh`, so its readiness check always failed; it is now a bash script, and a
  `set -e` trap when xrdp is absent was removed. CI updated.
- **CI:** keyring tests on Linux (inside a D-Bus session with GNOME Keyring), the
  "no keyring → refuse" test, and a Credential Manager round trip on Windows.

### Test results (Ubuntu 24.04, Rust 1.97.0)

| Check | Result |
|-------|--------|
| fmt, clippy (`-D warnings`), policy, cargo-deny, actionlint | ✅ PASS |
| Unit tests (debug / release) | ✅ PASS — 62/62 each |
| Live: save / replace / read / delete in GNOME Keyring; deleted = gone | ✅ PASS |
| Live: no plain-text password in the keyring files on disk | ✅ PASS (0 matches) |
| Live: no keyring available → save refused (I8) | ✅ PASS |
| Live: password saved in the keyring, read back, used to connect to the NLA host (T11) | ✅ PASS |
| Live RDP suite T1–T11 inside a keyring session | ✅ PASS — 10/10 |
| Windows Credential Manager + Windows Hello | ⛔ **NOT RUN** — Windows runner in CI |
| Android Keystore vault + biometric prompt | ⛔ **NOT RUN** — needs an Android device or emulator |
| Dart tests (random ids) and Flutter wiring | ⛔ **NOT RUN** — needs Flutter (CI) |

### Known issues (end-of-stage review)
- **KI-15:** On Linux the keyring is usually unlocked at login, so a saved password can be
  used without a new prompt. That is the platform's model; users who want a prompt every
  time should not save passwords on Linux. To be stated in the user documentation.
- **KI-16:** On Android the password passes through Dart after the Keystore prompt
  (extends KI-13). On Windows and Linux it stays in Rust.
- **KI-17:** Deleting a connection also forgets the trusted certificate for its host:port,
  even if another connection uses the same computer; that one will ask to approve again.
  Fix with pin management in Stage 7.
- **KI-18:** The plan's S13 option to turn off confirmation (with a warning) is not built;
  confirmation is always on. Settings arrive in Stage 7.
- **KI-19:** Saved passwords remain in the OS vault if the app is uninstalled on Windows or
  Linux; uninstall cleanup is part of Stage 10.

### Next step
Stage 6 — bidirectional text clipboard.

---

## Review of Stages 4–5 (29 Oct 2026)

| # | Problem found | Fix |
|---|---------------|-----|
| R14 | The Flutter UI test "valid form saves a profile" expected the password to be saved, but CI machines usually have no OS vault or Windows Hello, so it would always fail there | The test now accepts either correct outcome: saved, or not saved with the "Password not saved" message shown |
| R15 | When a saved password could not be read, the error said "the password was not saved", which describes saving, not reading | New title "Saved password couldn't be used" with advice to type it or save it again |
| R16 | When Windows Hello was not set up, saving reported "confirmation was cancelled" | Message now covers both cases (cancelled, or Windows Hello not set up) |
| R17 | The Save password text promised a confirmation prompt on every platform; on Linux there usually is none (KI-15) | Text now explains Windows/Android confirmation and Linux keyring protection |

Re-checked and found correct: session/TLS/pinning code paths, error mapping, the
bridge's reconnect-after-approval loop, vault id validation order, Kotlin vault logic
(time-bound key, retry after prompt, invalidation reset), CI job steps and environment,
dependency and secret checks.

---

## Stage 6 — Bidirectional Clipboard ✅ Engine verified live; Flutter/Android parts awaiting first CI run (29 Oct 2026)

### Objective
Two-way plain-text clipboard during authorised sessions only, with a per-connection
switch, one-direction options, a size limit, a visible indicator, and no other formats.

### Completed
- **Rust `clipboard`:** policy (direction + 1 MB limit, counted as UTF-16 bytes, the
  format sent over RDP) and a queue backend for IronRDP's clipboard channel. Only
  CF_UNICODETEXT is ever offered or requested; file transfer and other formats are never
  enabled. When clipboard sharing is off, the clipboard channel is **not even offered** to
  the host.
- **Session:** host → device text arrives as an event only if allowed and within the limit
  (too-large text is dropped with a notice); device → host text is offered, and only sent
  when the host pastes; text that just came from the host is not echoed back; everything
  stops when the session ends.
- **Bridge / Flutter:** per-connection mode (Off · Both · Only to remote · Only from remote,
  off by default); clipboard indicator in the session bar showing the direction; desktop
  checks the local clipboard every second while connected; Android only when the app
  returns to the foreground or via a "Send clipboard" button (Android shows a notice on
  every clipboard read); text from the host is marked **sensitive** on Android (hidden
  from clipboard previews on Android 13+).
- **Test-only host `tools/rdp-test-host`:** NLA plus a scriptable clipboard (offers a known
  text, records what it receives), built on the published `ironrdp-server` crate. Kept out
  of the app's workspace and never shipped. Its password comes from an environment variable.
  Started by `scripts/start-test-hosts.sh`; CI runs the clipboard tests against it.
- **Found and fixed during the stage:** wrong-password detection only knew Windows' status
  code; this host sends `0xC007030C` (a "logon denied" variant) and the app said "Something
  went wrong". Now classified correctly, with tests for both codes.

### Test results (Ubuntu 24.04, Rust 1.97.0)

| Check | Result |
|-------|--------|
| fmt, clippy (`-D warnings`), policy, cargo-deny, actionlint | ✅ PASS |
| Unit tests (debug / release) | ✅ PASS — 67/67 each |
| C1 host → device text (with accents) | ✅ |
| C2 device → host text (accents and emoji); same text not re-sent | ✅ |
| C3 clipboard off: nothing arrives from the host, nothing reaches the host | ✅ |
| C4 one direction only, both ways round | ✅ |
| C5 size limit: oversized host text dropped with a notice; oversized local text not offered | ✅ |
| C6 clipboard text never in logs (plain or UTF-16 hex) at the most detailed allowed level | ✅ |
| Host's own record: exactly the two allowed texts received, none of the "must not arrive" texts | ✅ |
| Whole live suite as CI runs it (host 10/10, clipboard 6/6, vault) | ✅ PASS |
| Clipboard against a **Windows** host | ⛔ **NOT RUN** — no Windows host here |
| Flutter clipboard sync, indicator, Android sensitive clipboard, Dart test | ⛔ **NOT RUN** — needs Flutter / Android (CI) |

### Known issues (end-of-stage review)
- **KI-20:** Changing a connection's clipboard mode takes effect on the next connection
  (when off, the channel is not offered at all, so it cannot be switched on mid-session).
- **KI-21:** On Android, text copied on the phone is sent only when the app returns to the
  foreground or the "Send clipboard" button is used, to avoid Android's clipboard-read
  notice on every check.
- **KI-22:** On desktop, the local clipboard is checked once a second while connected;
  the content is only advertised, and sent only when the remote computer pastes.
- **KI-24:** The test-only host (`tools/rdp-test-host`) uses `ironrdp-server` 0.13, which pins
  `zstd-safe` 7.x with two advisories (RUSTSEC-2026-0398, -0403; fixed in 8.2). The app does
  not use zstd at all (`cargo deny` on the app: clean). Recorded as test-only exceptions in
  `tools/rdp-test-host/osv-scanner.toml`; remove when the server crate updates.
- **KI-23:** The CI analysis step no longer fails on informational hints (warnings and
  errors still fail), because unused-import hints could not be checked without Flutter.

### Next step
Stage 7 — connection and profile management (saved to disk), pin management, Android
trackpad mode and pinch-zoom, settings for S5/S13.

---

## Review of Stage 6 (29 Oct 2026)

| # | Problem found | Fix |
|---|---------------|-----|
| R18 | A clipboard protocol error (e.g. the channel rejecting one message) ended the whole session | Clipboard protocol errors are now skipped; only a network failure ends the session |
| R19 | Offering empty clipboard text was reported as "too large" | Now "unchanged" (nothing to send); live test C5 checks it |
| R20 | The "text too large" notice could be shown after the session screen was closed | Checks that the screen is still open first |

After the fixes: unit tests 67/67; live connection suite 10/10 (in a keyring session);
live clipboard suite 6/6.


---

## Stage 7 — Connection and Profile Management ✅ Rust verified; Flutter screens awaiting first CI run (29 Oct 2026)

### Objective
Save connections, history and settings to disk (never passwords), manage trusted
computers, export/import, Wake-on-LAN and a readiness check, the S5/S13 settings, and
touch improvements (trackpad mode, pinch-zoom).

### Completed
- **Rust `fsutil`:** atomic writes (temp file + rename), a backup of the previous version,
  a 4 MB read limit, and setting a damaged file aside (`.damaged-<time>`) instead of
  deleting it.
- **Rust `profiles`:** connections (id, name, address, port, username, saved-password flag,
  clipboard mode, last used, Wake-on-LAN MAC) and history (last 100 outcomes, no secrets).
  Loading falls back to the backup, then starts empty with a notice; invalid entries are
  skipped and counted. Limits: 500 connections, 100 history entries.
- **Export / import:** export holds names, addresses, ports and usernames only — no
  passwords, ids, saved-password flags or history. Import validates every entry, gives new
  ids, and turns clipboard sharing **off** for everything imported. Windows/Linux use a
  file dialog; Android shows the text to copy and accepts pasted text.
- **Rust `settings`:** theme, block screenshots, clipboard default, confirm saved password,
  idle disconnect (5, 15, 30, 60, 120 minutes or never; default 30).
- **Rust `wol`:** MAC parsing (`:`, `-`, `.` or none), magic packet, sending, and a
  reachability check that only opens and closes a TCP connection.
- **Flutter:**
  - Connections, history and settings load at start-up and save on every change; a notice
    appears if the file was damaged or restored, or if saving fails.
  - Connection form: optional Wake-on-LAN MAC address (checked by the Rust core).
  - Connection menu: **Wake up** and **Check readiness** (computer answers on its port,
    certificate trusted, username set, password saved).
  - Settings: idle disconnect, **Windows Hello confirmation toggle with a warning before
    turning it off (KI-18 done)**, trusted computers (view fingerprints, forget), history,
    export, import.
  - Session: history outcome recorded; **re-trust after a certificate change** with a strong
    warning, which forgets the old fingerprint and reconnects so the new one must be checked
    (KI-11 done); **trackpad mode** (finger moves a visible pointer, tap clicks at the pointer,
    long press right-clicks) and **pinch-zoom** up to 4× in both modes, with "show the whole
    screen".
  - Deleting a connection keeps the trusted certificate if another connection uses the same
    computer and port (KI-17 done).
- **New tool `tools/dart-syntax`:** syntax-only check of Dart files (tree-sitter grammar,
  versions pinned in a lock file) for machines without Flutter. Runs in the pre-commit hook
  when installed. It does not check types or imports; `flutter analyze` in CI still does.

### Test results (Ubuntu 24.04, Rust 1.97.0)

| Check | Result |
|-------|--------|
| fmt, clippy (`-D warnings`), policy, cargo-deny | ✅ PASS |
| gitleaks (staged and full history) | ✅ PASS (see commit) |
| Unit tests | ✅ PASS — 90/90 (78 core + 12 bridge) |
| Profiles: save/load, backup fallback, damaged file set aside, limits, invalid entries skipped | ✅ |
| Export contains no password, id, saved-password flag or history; import turns clipboard off | ✅ |
| Wake-on-LAN packet sent and received over loopback; MAC formats; reachability | ✅ |
| Live connection suite, debug and release (in a keyring session) | ✅ PASS — 10/10 each |
| Live clipboard suite | ✅ PASS — 6/6 |
| Live vault: round trip; refuses to save with no keyring | ✅ PASS |
| Dart syntax of every app file (new checker) | ✅ PASS |
| OSV-Scanner | ⛔ **NOT RUN** here — downloading the scanner was blocked; runs in CI |
| Flutter build, `flutter analyze`, Dart unit tests (8 new), integration tests (3 new) | ⛔ **NOT RUN** — needs Flutter (CI) |
| Trackpad mode, pinch-zoom, Wake-on-LAN to a real computer, Windows Hello toggle | ⛔ **NOT RUN** — needs devices |

### Known issues (end-of-stage review)
- **KI-25:** No Dart compiler is available here. The new checker proves the Dart files parse,
  but types and imports are only checked by `flutter analyze` in CI. **The Flutter app has
  still never been compiled**; the first CI run may find errors to fix.
- **KI-26:** On Android, export shows the text (with a Copy button) and import takes pasted
  text, because the file picker is desktop-only here. The export holds no passwords, but
  copying it puts addresses and usernames on the clipboard.
- **KI-27:** Changing a connection's address leaves the old computer's trusted certificate in
  the list; it can be removed under Settings → Trusted computers.
- **KI-28:** Wake-on-LAN uses a local broadcast, so it works only on the same network as the
  computer (not through a VPN or router), and the computer must have it turned on.
- **KI-29:** The Windows Hello toggle only affects Windows. Android always asks (the Keystore
  key requires it); Linux relies on the keyring (KI-15).
- **KI-30:** `file_selector` ^1.0.3 was added without being able to resolve it here; the first
  CI run will confirm the version.

### Next step
Stage 8 — reliability: reconnecting after network drops, Android background behaviour
(decision 8), long-session and memory checks.

---

## Review of Stage 7 and earlier work (29 Oct 2026)

| # | Problem found | Fix |
|---|---------------|-----|
| R21 | **The app did not compile since Stage 6:** a stray `),` after the clipboard drop-down in the connection form (committed in `ed52b46`). Not caught because no Dart tools run here. | Removed. Added `tools/dart-syntax` and a pre-commit check; all Dart files and every earlier version of that file checked |
| R22 | On desktop, every mouse click and drag was sent **twice** (once by the mouse listener, once by the touch gesture handler) — since Stage 4 | The gesture handler now accepts only touch and pen; the mouse listener ignores them |
| R23 | After a pinch, the finger that stayed down started a drag (an unwanted click on the remote computer) | Fingers are counted; after a pinch nothing is sent until all fingers are lifted |
| R24 | The new integration test looked for `password"`, which also matches the allowed `save_password` key | Now checks for the exact `"password"` key |
| R25 | Leaving the session screen recorded no history entry, and recording it during the screen's disposal could rebuild widgets at a forbidden moment | Recorded when leaving, deferred until the screen is gone |
| R26 | Saving could silently drop connections over 500, and an import could exceed the limit | The form refuses a new connection at the limit; import adds only what fits and says so |
| R27 | Readiness check and shared-certificate check compared addresses case-sensitively, unlike the certificate store | Both ignore case now |

---

## Review of Stage 7 (29 Oct 2026, second pass)

| # | Problem found | Fix |
|---|---------------|-----|
| R28 | **Sessions could not open:** Stage 7 removed the `await` before `View.of(context)` in the session screen, so it ran inside `initState`, which Flutter forbids (an exception in debug builds, no session started) | The session now starts after the first frame |
| R29 | **Possible data loss:** if the connections file existed but couldn't be read (e.g. permissions), the app started with an empty list and the next change overwrote the file | After a read error nothing is saved in that run, and the notice says so |
| R30 | Two running copies of the app used the same temporary file name when saving, so one could corrupt the other's write; a failed write left the temporary file behind | Temporary names are unique per process and write; they are removed on failure; the folder is synced after the rename (Unix) |
| R31 | The "Connection lost" message promised automatic reconnecting, which did not exist yet | Built in Stage 8 (below); the message now describes what happens after the retries |

---

## Stage 8 — Reliability ✅ Engine verified live; Android service and Flutter parts awaiting first CI run (29 Oct 2026)

### Objective
Recover from network drops without risking account lockout, detect networks
that vanish silently, handle Android's background rules (decision 8), and check
that long sessions don't leak memory.

### Completed
- **Automatic reconnect (F13, SA-8):** after a network drop the session reconnects
  up to **3 times** (after 2 s, 5 s, 10 s) to the same address. The computer must present
  the **same trusted certificate**; a different one is blocked at once. A **wrong password,
  account problem, refused certificate or any other non-network error stops immediately**
  — it is never retried. The user can disconnect during the pauses. While reconnecting,
  the screen stays visible behind a "Connection lost. Reconnecting (1 of 3)…" overlay
  and input is ignored.
- **Silent drops:** TCP keepalive (15 s idle, 5 s probes, 3 probes) plus `TCP_USER_TIMEOUT`
  on Linux/Android, so a vanished network is noticed in about 30 s instead of never.
  Read time-outs and dead connections are now told apart per platform.
- **Android background (decision 8):** a foreground service with a notification keeps the
  session while the app is in the background; after **5 minutes** in the background the
  session ends with "Disconnected while in the background". The notification never names
  the computer (lock-screen privacy). Notification permission is asked once (Android 13+);
  if refused the session still works. Permissions now: network, foreground service
  (special use, with the stated reason), notifications.
- **Reconnect button** after a session ends for network, host, idle and background reasons
  (not after password, account or certificate problems).
- **Bridge refactor:** the session worker no longer depends on the Flutter stream type, so it
  can be tested directly; the reconnect rules are a separate function tested with fake results.
- **`tools/syntax-check`** (renamed from `dart-syntax`) now also checks Kotlin files.
- `SECURITY.md` describes reconnecting (and that the password stays in memory, wiped at the
  end, for the session) and the Android background behaviour.

### Test results (Ubuntu 24.04, Rust 1.97.0)

| Check | Result |
|-------|--------|
| fmt, clippy (`-D warnings`), policy, cargo-deny | ✅ PASS |
| Unit tests | ✅ PASS — 102/102 (82 core + 20 bridge) |
| Reconnect rules: wrong password / account problem → 1 attempt, stop; network errors → 3 attempts then "Connection lost"; changed certificate → blocked; unpinned certificate → no prompt; disconnect (and background disconnect) during a pause | ✅ |
| Only network errors may be retried (exact list) | ✅ |
| **L1** live: connection cut → "Reconnecting (1 of 3)" → reconnected without prompts; again after a second cut | ✅ |
| **L2** live: after a drop a different computer answers → certificate change shown, blocked | ✅ |
| **L3** live: network silently black-holed (iptables) → detected after **30.5 s**, reconnected when the network returned | ✅ |
| **L4** live: background disconnect ends with its own reason | ✅ |
| **L5** live: wrong password → ended, never retried | ✅ |
| **L6** live: 60 s session with constant screen changes — memory flat (49.4 MB → 49.4 MB) | ✅ |
| **L6** live for **10 minutes** (2,368 screen updates) — memory flat (33.9 MB → 33.9 MB) | ✅ |
| Whole live suite on the final code: connection 10/10 (debug and release), vault, no-vault refusal, clipboard 6/6, L1–L6 (silent drop detected after 30.6 s; no firewall rules left behind) | ✅ PASS |
| Dart and Kotlin syntax (all files) | ✅ PASS |
| actionlint (CI now also runs L1, L2, L4–L6) | ✅ PASS |
| Android foreground service, notification, 5-minute grace period, Reconnect button, overlay | ⛔ **NOT RUN** — needs Flutter / an Android device (CI) |
| Wi-Fi ↔ mobile data switch on a phone | ⛔ **NOT RUN** — needs a device |
| Reconnect against a **Windows** host | ⛔ **NOT RUN** — no Windows host here |

### Known issues (end-of-stage review)
- **KI-31:** To reconnect without asking, the password stays in the Rust worker's memory
  (wiped on drop) for the whole session, not only while connecting (extends KI-13).
- **KI-32:** If a session was started with a typed password, the Reconnect button asks for
  it again (the app does not keep typed passwords after a session ends).
- **KI-33:** Windows offers no setting for how long unacknowledged data is retried, so if
  input was being sent when the network vanished, Windows may take longer than 30 s to
  notice (Linux/Android: about 30 s either way).
- **KI-34:** The Android service type is "special use", which Google Play reviews by hand;
  the stated reason is in the manifest. Not an issue for direct APK installs.
- **KI-35:** If Android destroys the app's screen while in the background, the session ends
  at once rather than after 5 minutes (the session lives with the screen).
- **Test note:** a keyring file left by an earlier local run (made with a different random
  unlock password) made the keyring ask for a password and the vault tests fail; with a fresh
  keyring they pass. CI always starts fresh, so this only affects repeated local runs.
- **KI-25 (still open):** the Flutter app has still never been compiled; CI must run first.

### Next step
Stage 9 — security verification: run the full security checklist (SA-1…SA-9),
threat-model review, dependency audit, fuzzing of the parsers we own, and the
SF-2 report decision.

---

## Stage 9 — Security Verification ✅ No open critical or high issues (29 Oct 2026)

### Objective
Check every security requirement against the code, write the missing threat model,
fuzz our own parsers, run static analysis and licence audits, and settle SF-2.

### Review of Stage 8 (start of this stage)
Findings R35–R39 (details in `docs/SECURITY_REVIEW.md`): frame rectangles not clamped
like their pixels; the Reconnect-button rule duplicated in Dart (now in Rust, with an
**error-matrix test** covering every error: auto-retry, Reconnect button, warning);
**Android sessions that finished connecting in the background had no 5-minute limit**;
the clipboard was not re-offered after a reconnect; one drop-detection path was not
logged. Added live tests **L7** (no password in logs while reconnecting or after a
wrong password, three encodings) and **L8** (a healthy idle session is not dropped
for 90 s: no false alarms).

The two silent-drop detection times seen in Stage 8 are now explained by the logs:
**~30 s** = keepalive (read `TimedOut`); **10 s** = the time limit of a server
"reactivation" exchange that happened to be in progress (this test host performs one
right after connecting). Both are intended.

### Completed
- **`docs/THREAT_MODEL.md`** (SA-2): assets, attackers, trust boundaries, STRIDE table
  with the code or test behind every mitigation, accepted risks. It was due before
  Stage 4 (R32).
- **`docs/HOST_SETUP.md`** (SA-1, SA-4, S13, SF-6): Windows editions, NLA, separate
  non-admin account, lockout policy, updates, firewall limited to LAN/VPN, VPN with
  two-factor, Wake-on-LAN.
- **`docs/SECURITY_REVIEW.md`**: S1–S13, SA-1–SA-10 and SF-1–SF-6 each with status and
  evidence; findings R32–R39; audits.
- **Hostile-server hardening:** desktop sizes over 8K refused (R33, previously a possible
  17 GB allocation crash); pin file size-limited (R34).
- **Fuzzing (SA-3):** `rust/fuzz`, six libFuzzer targets for our parsers of untrusted
  input (server texts, clipboard bytes, certificates, desktop sizes, all app data and
  import files, form fields), built on the stable toolchain with coverage
  instrumentation; Rust bounds and overflow checks on. Seeds checked in; weekly CI job
  (60 s per target) keeps crashing inputs as artifacts. The core's `fuzzing` feature
  exposes internal parsers to this crate only.
- **Static analysis:** clippy (project lints) clean, also with the fuzzing feature;
  pedantic review found only style notes in our code (cast warnings are all in the
  generated FFI glue); no `unsafe` in our crates.
- **Dependencies and licences:** cargo-deny clean. The `rsa` advisory exception was
  re-checked for sspi 0.23.0 (its note still referred to 0.21.3).
- **SF-2:** reproduced and pinpointed: the `encrypt_message` debug span in sspi 0.23.0
  records the credentials buffer; still present in 0.23.1 (latest). A private report
  is drafted for the owner (kept **outside** this repository until disclosure).
- README and `SECURITY.md` link the new documents; `SECURITY.md` states that the
  independent review (SA-5) is still to be done.

### Test results (Ubuntu 24.04, Rust 1.97.0)

| Check | Result |
|-------|--------|
| fmt, clippy (`-D warnings`, also `--features fuzzing`), policy, cargo-deny, actionlint | ✅ PASS |
| Unit tests | ✅ PASS — 104/104 (84 core + 20 bridge) |
| Live: connection 10/10 (debug and release), vault, no-vault refusal, clipboard 6/6 | ✅ PASS |
| Live: bridge L1–L8 (reconnect, certificate on reconnect, silent drop, background reason, wrong password, memory, logs, idle) | ✅ PASS — 8/8 |
| Fuzzing, 10 minutes per target — server_text 32.1 M inputs, clipboard_text 165.3 M, certificate 20.3 M, desktop_size 770.7 M, files 28.4 M, form_fields 35.1 M (≈1.05 billion in total) | ✅ PASS — **no crashes** |
| OSV-Scanner, CodeQL | ⛔ **NOT RUN** here (CI only) |
| Flutter/Android parts of R36–R38 | ⛔ **NOT RUN** — need Flutter / a device |

### Known issues (end-of-stage review)
- **KI-36:** Dart package versions are not locked: `pubspec.lock` can only be generated
  with Flutter. Commit it from the first CI run.
- **KI-37:** The sspi 0.23.1 patch release is available (we use 0.23.0); it does not fix
  SF-2. Update together with IronRDP in a later dependency refresh.
- **KI-38:** Fuzzing here runs without AddressSanitizer (stable toolchain). Our crates have
  no `unsafe`, so Rust's own checks cover memory safety in the fuzzed code.
- **SA-5 (open):** an independent security review is required before the first public
  release.
- **KI-25 (still open):** the Flutter app has still never been compiled; CI must run first.

### Next step
Stage 10 — packaging and release: MSI, .deb, AppImage, portable ZIP (SA-10), signed APK,
checksums and SBOM (S8), and the steps to make the repository public.

---

## Review of Stage 9 (29 Oct 2026)

| # | Problem found | Fix |
|---|---------------|-----|
| R40 | The weekly OSV scan used `--config osv-scanner.toml`, which applies the root file to every folder, so the test host's and fuzzers' reviewed exceptions were ignored and the scan would have failed | Each folder's own file applies now (`--config` removed); the fuzz folder got its own file |
| R41 | `release-build.sh` listed the broad `$HOME` path rule last, and rustc uses the **last** matching rule, so the Cargo and Rustup rules never applied (paths became `/home/.cargo/...`; no user name leaked) | Broadest rule first; verified with the new `check-binaries.sh` |
| R42 | The app icons were still Flutter's template icons (the Flutter logo, a Google trademark) | Original icon (`tools/icon/make_icons.py`) for Android, Windows and Linux |

Also checked: npm tool dependencies (`npm audit`: 0 vulnerabilities); app identity in the
Windows and Linux runners is correct; all commits use the noreply identity.

---

## Stage 10 — Packaging and Release ⏳ Packages defined; first build waits for the GitHub repository (29 Oct 2026)

### Objective
Proper installers and portable versions for Windows, Linux and Android, signed
releases with checksums and an SBOM, complete documentation, and the steps to make the
repository public.

### Completed
- **Portable mode (SA-10):** Rust `portable.rs` decides where data lives: next to the
  Windows `.exe` when `portable.txt` is present (portable ZIP), or in a
  `<AppImage>.data` folder next to an AppImage; otherwise the normal per-user folders.
  A read-only drive falls back to the normal folders. Passwords are never stored there;
  ticking *Save password* in a portable copy shows a warning that it stays on this
  computer. Settings shows where a portable copy keeps its data.
- **Remove all saved passwords** (Settings) and the uninstall hook
  `--remove-saved-passwords`, which the Windows uninstaller runs (no window is shown).
- **Linux:** `.deb` (to `/opt`, launcher, app-menu entry, icons, AppStream metadata) and
  AppImage (portable). **Tested here** with a stand-in for the Flutter bundle: install →
  launch → upgrade 0.1.0 → 0.1.1 in place → uninstall left nothing behind.
- **Windows:** WiX v5 MSI — per-user by default (no administrator rights,
  `%LOCALAPPDATA%\Programs`), Start-menu shortcut, optional desktop shortcut, Add/Remove
  Programs entry with icon, in-place upgrade, downgrade refused, licence page from
  `LICENSE`, saved passwords removed on uninstall. Portable ZIP with `portable.txt` and
  a short README. Authenticode signing when a certificate is configured.
- **Android:** release APK signed with the owner's key from secrets; the workflow refuses
  to release a debug-signed APK and publishes the signing certificate's fingerprint.
- **Release workflow** (`.github/workflows/release.yml`): tag `vX.Y.Z` → version check →
  Linux, Windows and Android builds with local paths removed and checked → `.deb`
  installed and removed, MSI installed and uninstalled silently on clean runners → SBOM
  and `SHA256SUMS` → **draft** release for the owner to review. Only actions already
  pinned in this repository are used; the AppImage tool is pinned by version and SHA-256.
- **`scripts/check-binaries.sh`** (SA-9): fails if any release file contains a home or
  user-profile path (other than the anonymous CI runner's). Tested on our release
  library and with Linux- and Windows-style control files.
- **`scripts/make-sbom.sh`**: CycloneDX 1.5 SBOM for the Rust code (341 and 360
  components, all with licences), build folder replaced by `/src`; tested here.
- **Docs:** `docs/USER_GUIDE.md` (install, first connection, sessions, settings, portable,
  uninstall, troubleshooting) and `docs/RELEASING.md` (setup, release steps, manual
  device checklist, **going-public checklist**). README and security docs updated.

### Test results (Ubuntu 24.04, Rust 1.97.0)

| Check | Result |
|-------|--------|
| fmt, clippy (`-D warnings`), policy, cargo-deny, actionlint (3 workflows) | ✅ PASS |
| Unit tests | ✅ PASS — 107/107 (87 core + 20 bridge), incl. portable-mode rules |
| Dart and Kotlin syntax (all files) | ✅ PASS |
| `.deb` build, install, upgrade, uninstall (stand-in bundle) | ✅ PASS |
| Local-path check on the release Rust library; controls caught | ✅ PASS (after R41) |
| SBOM generation without local paths | ✅ PASS |
| Icons drawn and rendered (PNG and SVG) | ✅ |
| Flutter release builds, AppImage, MSI, portable ZIP, signed APK, install tests in the workflow | ⛔ **NOT RUN** — need Flutter, WiX/.NET and Windows (CI) |
| Manual device checklist in `docs/RELEASING.md` | ⛔ **NOT RUN** — needs devices |

### Known issues (end-of-stage review)
- **KI-39:** The WiX file, PowerShell scripts and release workflow have only been checked
  for syntax here (XML, actionlint); their first real run is the first tagged release
  (or "Run workflow" by hand, which builds without publishing).
- **KI-40:** Uninstall cleanup removes the passwords of connections in the connections
  file. If that file was deleted by hand first, its vault entries stay (the vault
  cannot be listed by app).
- **KI-41:** A `.deb` cannot remove passwords from users' keyrings on uninstall; the user
  guide says to use *Remove all saved passwords* first.
- **KI-42:** Release builds use Flutter's current stable version, not a pinned one.
- **KI-43:** Without a code-signing certificate, Windows files are unsigned and
  SmartScreen warns; the release notes and user guide explain how to check checksums.
- **KI-44:** The AppImage is built on Ubuntu 22.04, so it needs glibc 2.35 or newer.
- **SA-5 (open):** independent security review before going public.
- **KI-25 (still open):** the Flutter app has still never been compiled.

### Next step
Owner: create the private repository `secure-rdp-viewer` and push; then CI's first run
(fix what it finds, commit `pubspec.lock` from its artifacts), then "Run workflow" on
Release to build all packages without publishing, then the device checklist. Going
public only after SA-5 and the checklist in `docs/RELEASING.md`.

---

## Privacy check before going public (29 Oct 2026)

Checked the files that would be published and the whole git history:
gitleaks (project rules over every commit; default rules with no exceptions over every
file): clean. No email addresses other than `example.com` placeholders; every commit is
by `Bensal <Bensalk@users.noreply.github.com>`; no personal names, organisations, local
folder paths, real IP addresses or hostnames; no personal GitHub accounts in links;
no real passwords anywhere. Changed: the one fixed dummy password in an app test is now
random per run (SA-9). Added: "Created by Bensal" as the last line of Settings → About,
in small plain text. Open point for the owner: each commit message ends with a link to
the private Claude session used to build the project.
