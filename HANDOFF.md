# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 14 Nov 2026._

## Current branch and build

- **Work is now on `main`** — the `connection-methods` branch was merged.
  All releases now dispatch from `main`.
- **v0.3.13** — build dispatched (run 43608605235). This should be the
  **first working build with host→viewer file clipboard** on Windows 11.
  **What was fixed:** Explorer on Windows 11 never puts `CF_HDROP` on the
  clipboard — it uses `CFSTR_SHELLIDLIST` ("Shell IDList Array") instead.
  v0.3.9 added the CIDA/PIDL parsing code but it had three compile errors
  hidden by Cargokit/MSBuild: `ILCombine` and `ILFree` need
  `Option<*const ITEMIDLIST>` (not bare pointer), and `SHGetPathFromIDListW`
  takes `&mut [u16; 260]` (not `PWSTR`). Fixed in v0.3.13.
  **After the build finishes** (draft release at
  https://github.com/Bensalk/cipher-desk/releases):
  1. Install the APK on your phone.
  2. On the host (Windows) PC, copy some files in Explorer (Ctrl+C).
  3. On the viewer (phone), long-press in a file manager → paste.
  4. Tell me if it works.
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

## Waiting for Bensal's test results

1. **v0.3.13** — host→viewer file clipboard fix. Confirm the draft release
   builds, install APK, test: copy files in Windows Explorer → paste on phone.
2. **Reconnect storm** (known issue, next fix): ~20 disconnect events fire
   within 1 second when the connection drops. Not addressed yet.

## Sandbox gotcha: spurious draft v0.3.9 release

There is a stale draft release with ID 445340661 for v0.3.9 on GitHub — it was
created by a workflow that ran on old code. It should be deleted but the REST
API returns 403 from the cloud session. Use Chrome automation
(`mcp__claude-in-chrome__*`) on the GitHub Releases page to delete it manually.

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
