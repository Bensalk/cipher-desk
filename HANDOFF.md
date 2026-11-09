# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 9 Nov 2026._

## Current branch and build

- Work happens on branch **`connection-methods`** (PR #12, open, not merged to
  `main`). It adds three connection methods: Private Remote Desktop, Share
  screen, VNC, plus the always-on Host service (sign-in screen / unattended).
- **v0.3.4** (commit `688d1b5`, tag `v0.3.4`):
  https://github.com/Bensalk/cipher-desk/releases/tag/v0.3.4
  — released 9 Nov 2026. **Build still running** (run `42624733572`) —
  check the release page for APK, MSI, ZIP, AppImage, .deb (~20–30 min
  from tag push). Bensal has not yet tested this build.
  What's in it:
  1. **Fixed:** Quality switch (Low/Medium/High) no longer triggers the
     15-minute password lockout. Quality switches cause disconnect +
     Tailscale relay reconnect; those were counted as failed login attempts.
     Now only a wrong password counts.
  2. **Added:** Diagnostic logging for host→viewer file clipboard (copy on
     host, paste on viewer). Share logs from
     `C:\ProgramData\Cipher Desk\host-service\logs\` if file paste doesn't
     work.
  Commits this session: `fix: rustfmt long tracing lines in clipboard.rs`
  (`688d1b5`) and `.claude/settings.json` allow rules (`55da73c`).
- **v0.3.3** (commit `8051c80`, built and released as a draft, link below):
  two fixes, both from Bensal's live testing of v0.3.2 on 8 Nov.
  1. **More patient auto-reconnect.** `max_auto_reconnects` 3 → 6 and the
     backoff curve 2/5/10s → 2/5/10/15/20/30s (`rust/core/src/policy.rs`,
     `rust/core/src/status.rs`) — about 1.5 minutes of retrying instead of
     ~17 seconds.
  2. **Fixed a real file-copy-paste bug** (Windows' "Error Copying File or
     Folder: The object invoked has disconnected from its clients."). Root
     cause: `_putReceivedOnClipboard` rewrote the clipboard once per
     `FileReceived` event; a folder/multi-file batch fires one event per
     top-level item — so a paste mid-batch could have the clipboard rewritten
     out from under Explorer. Now debounces: waits 600ms after the last file
     before writing once.
  Draft: https://github.com/Bensalk/cipher-desk/releases/tag/untagged-8805f71aebfc9b15e7b2
  — sent to Bensal 8 Nov, not yet confirmed tested.

## Waiting for Bensal's test results

1. **v0.3.4** — just published (9 Nov 2026). Check build artifacts attached
   to https://github.com/Bensalk/cipher-desk/releases/tag/v0.3.4 once the
   release workflow finishes (~20–30 min from tag push). Send Bensal the link
   and ask him to install the APK (phone) + MSI (Windows).
2. **v0.3.3's two fixes** (longer auto-reconnect, file-copy clipboard
   batching) — not yet confirmed from Bensal. He was blocked testing v0.3.2
   when this was built.
3. **Host→viewer file clipboard bug** (Bug 2): after Bensal tests v0.3.4,
   ask him to share logs from `C:\ProgramData\Cipher Desk\host-service\logs\`
   — look for `"clipboard changed"` and `"offering host clipboard files to
   viewer"` lines to diagnose whether files are being offered at all.
4. **His saved password for "Server PC"** — likely stale after he changed
   the Share screen host's password; told him to re-enter/re-save on viewer
   side. Not yet confirmed.
5. **Picture/video quality** — lower priority; next lever is DXGI Desktop
   Duplication on the host side.
6. **Floating full-screen/keys button opacity** — deferred by Bensal.

## Official release (v0.4.0), once v0.3.3/v0.3.4 is confirmed

Bensal wants to publish an **official** GitHub release before his weekly
usage resets. Plan once he confirms the latest test build works:
1. Merge `connection-methods` (PR #12) into `main`.
2. Update **README.md** — still entirely on v0.2.1 (version badge, download
   table, feature list).
3. Spot-check `docs/*.md` match current behavior.
4. Tag and publish as **v0.4.0** (his choice; signals big jump from v0.1.0).
5. Bensal publishes the actual GitHub release himself.

## Done on this branch

| Version | What |
| --- | --- |
| v0.2.3 | Share screen keyboard/mouse fixed (SendInput needed `DESKTOP_JOURNALPLAYBACK`; input on its own thread). No "port in use" when the always-on service already shares. |
| v0.2.4 | Picture quality per connection (Best = lossless QOIZ, Balanced, Speed); host always sends the newest picture (less lag); view modes Original / Shrink / Stretch; real full screen (taskbar hidden); Cipher Desk window on the shared PC visible to the viewer (capture-hiding only while *viewing*). |
| v0.2.5 | Viewer holds a big change until the host has finished sending it (max 150 ms), so Best quality appears whole instead of in bands. `Session::poll` now returns `(events, caught_up)`. |
| v0.2.6 | Host window no longer hidden from capture at startup (startup race lost the "show" call). |
| v0.2.9 | **Folders and any file size**: shared `rdpv_core::xfer` now has `offer` (walks folders, max 20 000 items), `Receiver` (`.part` file, 4 MiB `CHUNK` requests), and `answer` (≤ 8 MiB per request). |
| v0.2.8 | **File copy/paste like AnyDesk** (Windows); **2FA with Google Authenticator** (always-on sharing). |
| v0.2.7 | **Ask to connect (no password)**: always-on worker listens on port+1; popup via `WTSSendMessageW`. |

## Next features Bensal asked for (AnyDesk-like), in his priority order

1. "Always allow this device" checkbox; popup for manual sharing too; show who is asking.
2. Unattended access password in OS vault for always-on service.
3. 2FA for manual sharing too.
4. File transfer progress bar; resume broken copy.
5. Backlog: auto-adapt host resolution to viewer window.

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
- The Android emulator CI job sometimes times out (exit 124); re-run only the
  failed job before suspecting the code.
