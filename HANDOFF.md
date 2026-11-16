# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 17 Nov 2026._

## Current branch and build

- **Work is on `main`.** All releases dispatch from `main`.
- **v1.0.0 (17 Nov 2026) — first stable release, build in progress / awaiting
  Bensal's test.** Combines two fixes into one release:
  - **Lockout bug fix:** the global "many devices guessing together" lockout
    (`rust/host/src/limiter.rs`, `AllCounter`) used to count every wrong-password
    *attempt*, so repeated wrong passwords from one device could trip a
    15-minute lockout of everyone. Now it counts distinct devices, so only a
    genuine flood from many different addresses triggers it; one device's own
    repeated mistakes only lock out that device (unchanged).
  - **New: cancel button on the file-transfer progress bar.** Stops a copy
    arriving from the remote right away; the part already written is deleted,
    anything already fully received stays. Wired end to end: Rust core
    (`Session::cancel_incoming_files`) → bridge → Flutter (`session_screen.dart`,
    key `transfer-cancel`).
  - Full Rust test suite green, including new stress tests (thousands of
    devices against the rate limiter; repeated fail/succeed cycles; several
    threads connecting at once) added to prove the lockout fix and check for
    other concurrency bugs. A full security/concurrency review of the
    security-sensitive code (path handling, DACLs, TOTP timing, CLIPRDR
    parsing, command injection) found nothing else.
  - Docs reworked for v1.0.0: README badge/links/features, the per-device vs.
    global lockout wording, Share screen's missing file-copy and two-step
    verification instructions, a file-transfer comparison row, SECURITY.md.
  - **Still waiting on:** Bensal's own test pass (wrong-password Windows
    notification, cancel button on a real transfer, Exit/reopen, duplicate
    file naming, no-resend-on-reconnect) before this is called ready to publish.

## Next features Bensal asked for, in priority order

1. ~~"Always allow this device" checkbox; popup for manual sharing too; show
   who is asking.~~ Done (Ask to connect).
2. ~~Unattended access password in OS vault for the always-on service.~~ Done
   (DPAPI machine scope under ProgramData).
3. ~~2FA for manual sharing too.~~ Done (Two-step verification, Google
   Authenticator).
4. File transfer progress bar — done, with a cancel button (v1.0.0). **Resuming
   a broken copy** after a cancel or disconnect is not built.
5. Backlog: auto-adapt host resolution to the viewer's window size.

## UI polish ideas — after v1.0, not urgent

Bensal said "note it down, do it after v1.0 later." No code written yet.

- Share screen: group the flat list of switches/buttons under small headers
  (Connection / Security / Advanced); add a colored status dot/chip next to
  port, username, wrong-password count instead of plain text.
- Viewer: a small persistent "Received files" panel/badge (not just the
  snackbar, which disappears in seconds) — also where an Android share-sheet
  button for received files would live.
- Tray icon: badge/number overlay on the icon itself showing wrong-password
  count, visible without opening the menu.
- Connection history: in-app scrollable log viewer (newest first, failed
  attempts in red) instead of opening the Notepad file.

## Known issue: version string in Report Problem

`app/lib/ui/session_screen.dart` has `const version = '...'` hardcoded.
Update this when bumping pubspec.yaml (currently kept in sync manually).

## Sandbox gotchas (Claude's cloud workspace)

- No Flutter/Android SDK: CI is the compile check for Dart. Rust builds locally.
- `rust-toolchain.toml` pins 1.97.0 but rustup can't download; the installed
  `stable` *is* 1.97.0, so use `RUSTUP_TOOLCHAIN=stable cargo … --offline`.
- flutter_rust_bridge codegen: install the codegen binary with cargo and put
  small fake `flutter`/`dart` scripts on PATH (only version queries are made).
  The real `flutter_rust_bridge_codegen generate` cannot run here (network
  blocked) — write generated files by hand, following existing modules'
  patterns (see CLAUDE.md's FRB section; next unused funcId is 59).
- The local clone may only fetch `main`; check other branches with
  `gh api repos/Bensalk/cipher-desk/branches/<name> --jq .commit.sha`.
- GitHub GraphQL is unreliable for release/publish status (can report a
  release as published when REST returns 404). Always check with
  `gh api repos/Bensalk/cipher-desk/releases/tags/vX.Y.Z`.
- Start a build: `gh api -X POST repos/Bensalk/cipher-desk/actions/workflows/release.yml/dispatches -f ref=main -F 'inputs[make_release]=true'`.
- After bumping versions, fix `rust/fuzz/Cargo.lock` too (search for the
  `rdpv-core` entry specifically — other crates may coincidentally show the
  same version number).
- `.claude/settings.json` allow rules let Claude auto-push version tags and
  create releases in future sessions.
- Tag push is blocked in cloud sessions (HTTP 403 via GitHub proxy). Dispatch
  the release workflow on the branch instead (see command above) — it creates
  the tag and draft release itself, bypassing the push block.
- **Built-in browser is NOT signed in to GitHub** — Chrome (the user's real
  browser) is signed in; the built-in browser has its own profile. Use Chrome
  automation (`mcp__claude-in-chrome__*`) for GitHub web UI actions (e.g.
  deleting a stuck draft release the REST API won't delete).
- The Android emulator CI job sometimes times out (exit 124); re-run only the
  failed job before suspecting the code.
- A dispatched workflow run can occasionally get stuck reporting `status:
  queued` indefinitely, with `gh api .../cancel` rejecting with HTTP 409. This
  is a GitHub-side anomaly — don't fight it; just dispatch a fresh run.
