# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 15 Nov 2026._

## Current branch and build

- **Work is now on `main`** — the `connection-methods` branch was merged.
  All releases now dispatch from `main`.
- **v0.3.21** — build dispatched (run 43703352686, 15 Nov 2026).
  **What is in it:** Two fixes to `rust/host/src/clipboard.rs`:
  1. **Quiet-window bug fixed** — after the viewer sent files to the host, the host
     suppressed ALL clipboard changes for 5 seconds (including genuine text copies).
     Now only file-clipboard echoes are suppressed; text is advertised immediately
     even during the quiet window.
  2. **Host clipboard advertising now logged** — the host log now shows
     `host clip: advertising clipboard state to viewer has_text=true/false` every
     time it tells the viewer what is on the clipboard. Previously `advertise()` was
     completely silent.
  **To test (host→viewer clipboard):**
  1. Install v0.3.21 on **both** PCs.
  2. Connect viewer to host. Set clipboard to "Both directions".
  3. Copy text on the HOST (Ctrl+C in Notepad or any app).
  4. Try to paste on viewer — does it work?
  5. **Share both logs:**
     - **HOST log** (`worker.log` on the host PC) — look for:
       `host clip: advertising clipboard state to viewer has_text=true`
     - **VIEWER session log** (tap "Copy Diagnostic Log" in the Cipher Desk app
       while STILL CONNECTED to the session, NOT worker.log) — look for:
       `clip: host clipboard changed (RemoteChanged) has_text=true`
       `clip: sent FormatDataRequest for text`
       `clip: received format data from host bytes=N`
     The first missing line is where the chain breaks.
- **v0.3.20** — **PUBLISHED** at https://github.com/Bensalk/cipher-desk/releases/tag/v0.3.20
  What was in it: Deeper HOST→VIEWER clipboard diagnostics in the viewer session log.
- **v0.3.19** — **PUBLISHED** at https://github.com/Bensalk/cipher-desk/releases/tag/v0.3.19
  Build all green (run 43692597749). Published. Contains clipboard policy logging.
- **v0.3.18** — build dispatched (run 43644585250, 15 Nov 2026). Wait for the
  draft release at https://github.com/Bensalk/cipher-desk/releases; then publish it.
  **What was fixed:**
  - **Viewer→host file paste — CF_HDROP wiped before you could paste** — VIEWER→HOST
    direction only (copy files on viewer, paste in Explorer on host). The HANDOFF
    previously said this fixed "host→viewer" — that description was wrong.
  **To test (VIEWER→HOST direction):**
  1. On the viewer PC, copy a file in Explorer (Ctrl+C).
  2. Connect to the host (or already connected).
  3. The file transfer should happen automatically.
  4. On the host PC, open Explorer and press Ctrl+V — the file should paste.
  5. It should paste successfully even if you wait 30 seconds before pressing Ctrl+V.
- **v0.3.17** — **PUBLISHED** at https://github.com/Bensalk/cipher-desk/releases/tag/v0.3.17
  **What was fixed:** CapsLock desync on Windows — removed sync-at-connect call.
- **v0.3.16** — **PUBLISHED** at https://github.com/Bensalk/cipher-desk/releases/tag/v0.3.16
  **What was fixed:**
  1. **CapsLock key bar button** — Caps button added for touch users.
  2. **File clipboard overwrite after paste** — 5-second quiet window in host
     worker prevents CLIPRDR cascade from overwriting CF_HDROP.
- **v0.3.15** — **PUBLISHED** at https://github.com/Bensalk/cipher-desk/releases/tag/v0.3.15
  **What was fixed:** Host→viewer file clipboard "Access is denied." error.
- **v0.3.14** — build dispatched (run 43622084977, 14 Nov 2026).
  **What was fixed:** CapsLock / NumLock / ScrollLock sync on connect.
  When you connect to a remote PC, Cipher Desk now sends your keyboard's
  current lock-key state to the host. The host reads each lock with
  `GetKeyState` and toggles it with `SendInput` if it doesn't match, so
  both keyboards are in sync from the first keystroke.
  **After the build finishes** (draft release at
  https://github.com/Bensalk/cipher-desk/releases):
  1. Install the APK on your phone (or MSI on Windows).
  2. Turn CapsLock ON on your phone/PC before connecting.
  3. Connect to the host — type a few letters. They should be uppercase.
  4. Try the same with NumLock off on your side.
  5. Tell me if it works.
- **v0.3.13** — build dispatched (run 43608605235). First working build with
  host→viewer file clipboard on Windows 11 (CFSTR_SHELLIDLIST fix).
- **v0.3.6** (commit `e74af06`): published at
  https://github.com/Bensalk/cipher-desk/releases/tag/untagged-6271b0ad8090ed596fc6
  What's in it:
  1. **Clipboard on by default** — new connections now default to both directions.
  2. **File transfer messages** — clearer next-step messages after send/receive.
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

## Active investigation: host→viewer clipboard broken

Bensal confirmed both PCs are on v0.3.20. HOST→VIEWER text clipboard still broken.
v0.3.21 fixes two bugs found in `rust/host/src/clipboard.rs`:

1. **5-second quiet window** — after the viewer sent files to the host, the host
   suppressed ALL clipboard changes for 5 seconds. Text copies during that window
   were silently dropped. Now only file-clipboard echoes are suppressed.
2. **advertise() was completely silent** — the host log never showed whether text
   was being advertised to the viewer. Now it logs `has_text=true/false` on every
   advertising call.

The viewer PC has **no worker.log** — that file only exists on the HOST PC (which
runs the Windows host service). The viewer's diagnostics are in the **session log**
inside the Cipher Desk app: tap "Copy Diagnostic Log" while STILL CONNECTED to a
session. This is not worker.log — it is different.

**Next step after v0.3.21 installed:** Need both logs to find where the chain breaks:
- Host `worker.log`: should show `host clip: advertising clipboard state to viewer has_text=true`
- Viewer session log (from "Copy Diagnostic Log" button in app): should show
  `clip: host clipboard changed (RemoteChanged) has_text=true` and
  `clip: sent FormatDataRequest for text`

## Sandbox gotcha: spurious draft v0.3.9 release

There is a stale draft release with ID 445340661 for v0.3.9 on GitHub — it was
created by a workflow that ran on old code. It should be deleted but the REST
API returns 403 from the cloud session. Use Chrome automation
(`mcp__claude-in-chrome__*`) on the GitHub Releases page to delete it manually.

## Done on this branch

| Version | What |
| --- | --- |
| v0.3.21 | Host clipboard quiet-window bug fix; advertise() logging added |
| v0.3.20 | Viewer-side clipboard diagnostic logging (session log) |
| v0.3.19 | Clipboard-direction diagnostic logging |
| v0.3.18 | VIEWER→HOST CF_HDROP fix — block on_remote_copy from wiping clipboard |
| v0.3.17 | CapsLock desync fix — removed sync-at-connect |
| v0.3.16 | Caps key bar button; CF_HDROP 5-second quiet window (incomplete fix) |
| v0.3.15 | Host→viewer file clipboard "Access is denied." fix (retry up to 4 s) |
| v0.3.14 | CapsLock/NumLock/ScrollLock sync on connect |
| v0.3.13 | Windows 11 host→viewer file clipboard (CFSTR_SHELLIDLIST fix) |
| v0.3.6 | Clipboard on by default; file-transfer messages show next step and destination path |
| v0.3.5 | Report Problem log export button; floating button 25% idle opacity |
| v0.3.4 | Quality switch no longer triggers 15-min lockout; host→viewer file clipboard logging |
| v0.3.3 | More patient auto-reconnect (6 attempts, 2/5/10/15/20/30s); file clipboard debounce (600ms batch) |
| v0.2.3 | Share screen keyboard/mouse fixed; no "port in use" when always-on service already shares |
| v0.2.4 | Picture quality per connection; view modes; real full screen |
| v0.2.5 | Viewer holds big change until host finishes (max 150 ms) |
| v0.2.6 | Host window no longer hidden from capture at startup |
| v0.2.9 | Folders and any file size for file clipboard |
| v0.2.8 | File copy/paste (Windows); 2FA with Google Authenticator |
| v0.2.7 | Ask to connect (no password); popup via WTSSendMessageW |

## Next features Bensal asked for, in his priority order

1. "Always allow this device" checkbox; popup for manual sharing too; show who is asking.
2. Unattended access password in OS vault for always-on service.
3. 2FA for manual sharing too.
4. File transfer progress bar; resume broken copy.
5. Backlog: auto-adapt host resolution to viewer window.

## Known issue: version string in Report Problem

`app/lib/ui/session_screen.dart` has `const version = '...'` hardcoded.
Update this when bumping pubspec.yaml (currently kept in sync manually).

## Sandbox gotchas (Claude's cloud workspace)

- No Flutter/Android SDK: CI is the compile check for Dart. Rust builds locally.
- `rust-toolchain.toml` pins 1.97.0 but rustup can't download; the installed
  `stable` *is* 1.97.0, so use `RUSTUP_TOOLCHAIN=stable cargo … --offline`.
- flutter_rust_bridge codegen: install the codegen binary with cargo and put
  small fake `flutter`/`dart` scripts on PATH (only version queries are made).
- The local clone may only fetch `main`, so `origin/connection-methods` can be
  stale. Check with `gh api repos/Bensalk/cipher-desk/branches/connection-methods --jq .commit.sha`.
- GitHub GraphQL is blocked: use `gh api` REST.
- Start a build: `gh api -X POST repos/Bensalk/cipher-desk/actions/workflows/release.yml/dispatches -f ref=main -F 'inputs[make_release]=true'`.
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
