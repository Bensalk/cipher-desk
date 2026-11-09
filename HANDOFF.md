# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 9 Nov 2026._

## Current branch and build

- Work happens on branch **`connection-methods`** (PR #12, open, not merged to
  `main`). It adds three connection methods: Private Remote Desktop, Share
  screen, VNC, plus the always-on Host service (sign-in screen / unattended).
- **v0.3.6** (commit `5dce261`, tag not yet created):
  **Needs build** — push tag `v0.3.6` via Chrome (tag push blocked in cloud).
  What's in it:
  1. **Clipboard on by default** — new connections now default to both
     directions. Existing connections saved with clipboard off show a 60-second
     "Turn on now" banner at session start.
  2. **File transfer messages** — after sending a file: "press Ctrl+V in a
     folder on the remote". After receiving: shows the full saved path on
     Android/Linux; "press Ctrl+V in any folder" on Windows.
- **v0.3.5** (commit `ac88014`, tag `v0.3.5`):
  Build dispatched at ~15:06 UTC 9 Nov 2026 (run 42641271410).
  Draft release will appear at https://github.com/Bensalk/cipher-desk/releases
  — Bensal publishes it.
  What's in it:
  1. **Report Problem button** — after any connection error, a "Report Problem"
     button appears on the error screen. Exports a plain-text log (last ~1000
     events: connects, disconnects, reconnect attempts, quality switches,
     clipboard events). Android: share sheet. Windows/Linux: file-save dialog.
     No passwords or screen content ever included.
  2. **Floating control button opacity** — the overlay button in full-screen
     mode is 25% opaque when idle; snaps to 100% on hover, drag, or tap.
- **v0.3.4** (commit `688d1b5`, tag `v0.3.4`):
  https://github.com/Bensalk/cipher-desk/releases/tag/v0.3.4
  — released 9 Nov 2026. Bensal has not confirmed tested yet.
  What's in it: Quality switch (Low/Medium/High) no longer triggers 15-min
  lockout; diagnostic logging for host→viewer file clipboard.

## Waiting for Bensal's test results

1. **v0.3.4** — install APK (phone) + MSI (Windows), test quality switch
   (Low → Medium → High should NOT lock out for 15 min). Release:
   https://github.com/Bensalk/cipher-desk/releases/tag/v0.3.4
2. **v0.3.3's two fixes** (longer auto-reconnect, file-copy clipboard
   batching) — not yet confirmed from Bensal.
3. **Host→viewer file clipboard bug** (Bug 2): ask Bensal to share logs from
   `C:\ProgramData\Cipher Desk\host-service\logs\` — look for
   `"clipboard changed"` and `"offering host clipboard files to viewer"` lines.
4. **His saved password for "Server PC"** — likely stale after changing the
   Share screen host's password; told him to re-enter/re-save on viewer side.

## Official release (v0.4.0), once v0.3.4 is confirmed

Plan once Bensal confirms the test build works:
1. Merge `connection-methods` (PR #12) into `main`.
2. Spot-check `docs/*.md` match current behavior.
3. Tag and publish as **v0.4.0**.
4. Bensal publishes the GitHub release himself.

## Done on this branch

| Version | What |
| --- | --- |
| v0.3.6 | Clipboard on by default; file-transfer messages show next step and destination path |
| v0.3.5 | Report Problem log export button; floating button 25% idle opacity |
| v0.3.4 | Quality switch no longer triggers 15-min lockout; host→viewer file clipboard logging |
| v0.3.3 | More patient auto-reconnect (6 attempts, 2/5/10/15/20/30s); file clipboard debounce (600ms batch) |
| v0.2.3 | Share screen keyboard/mouse fixed; no "port in use" when always-on service already shares |
| v0.2.4 | Picture quality per connection; view modes; real full screen |
| v0.2.5 | Viewer holds big change until host finishes (max 150 ms) |
| v0.2.6 | Host window no longer hidden from capture at startup |
| v0.2.9 | Folders and any file size for file clipboard |
| v0.2.8 | File copy/paste like AnyDesk (Windows); 2FA with Google Authenticator |
| v0.2.7 | Ask to connect (no password); popup via WTSSendMessageW |

## Next features Bensal asked for (AnyDesk-like), in his priority order

1. "Always allow this device" checkbox; popup for manual sharing too; show who is asking.
2. Unattended access password in OS vault for always-on service.
3. 2FA for manual sharing too.
4. File transfer progress bar; resume broken copy.
5. Backlog: auto-adapt host resolution to viewer window.

## Known issue: version string in Report Problem

`app/lib/ui/session_screen.dart` has `const version = '0.3.6'` hardcoded in
`_reportProblem`. Update this when bumping pubspec.yaml, or add
`package_info_plus` to read it at runtime.

## Sandbox gotchas (Claude's cloud workspace)

- No Flutter/Android SDK: CI is the compile check for Dart. Rust builds locally.
- `rust-toolchain.toml` pins 1.97.0 but rustup can't download; the installed
  `stable` *is* 1.97.0, so use `RUSTUP_TOOLCHAIN=stable cargo … --offline`.
- flutter_rust_bridge codegen: install the codegen binary with cargo and put
  small fake `flutter`/`dart` scripts on PATH (only version queries are made).
- The local clone may only fetch `main`, so `origin/connection-methods` can be
  stale. Check with `gh api repos/Bensalk/cipher-desk/branches/connection-methods --jq .commit.sha`.
- GitHub GraphQL is blocked: use `gh api` REST.
- Start a build: `gh api -X POST repos/Bensalk/cipher-desk/actions/workflows/release.yml/dispatches -f ref=connection-methods -F 'inputs[make_release]=true'`.
- After bumping versions, fix `rust/fuzz/Cargo.lock` too.
- `.claude/settings.json` allow rules let Claude auto-push version tags and
  create releases in future sessions.
- Tag push blocked in cloud sessions (HTTP 403 via GitHub proxy). Workaround:
  use Chrome automation on GitHub web UI to create the tag + release.
- **Built-in browser is NOT signed in to GitHub** — Chrome (the user's real
  browser) is signed in; the built-in browser has its own profile. Use Chrome
  automation (`mcp__claude-in-chrome__*`) for GitHub web UI actions.
- The Android emulator CI job sometimes times out (exit 124); re-run only the
  failed job before suspecting the code.
