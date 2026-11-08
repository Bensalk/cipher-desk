# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 7 Nov 2026._

## Current branch and build

- Work happens on branch **`connection-methods`** (PR #12, open, not merged to
  `main`). It adds three connection methods: Private Remote Desktop, Share
  screen, VNC, plus the always-on Host service (sign-in screen / unattended).
- **v0.3.0** (commit `1122d0d`): host service DPI-aware (`win::make_dpi_aware`, fixes cut-off picture/wrong clicks at 125–150 % scale); clipboard setting renamed "Copy and paste (text and files)", on by default for new Share screen connections, snackbar when off; clipboard opens retried 20×25 ms + 150 ms settle after a change (host Rust + viewer C++/Dart) because Explorer holds the clipboard right after Ctrl+C. Release run 42463588907: Windows/Linux built, Android failed 3× on GitHub 504 downloads (Kotlin/Gradle), retry scheduled; no draft yet. Bensal's test 8 Nov (v0.2.9): file copy did NOT work either way, text did; lag was Tailscale relay (DERP, hard NAT `MappingVariesByDestIP: true`, no UPnP), parked by him.
- Previous test build: **v0.2.9** (folders + files of any size in copy/paste;
  contains v0.2.7 and v0.2.8). Test order: v0.2.6, then v0.2.9.
  v0.2.9 draft (commit `3d42b05`, also has the Google Authenticator QR code):
  https://github.com/Bensalk/cipher-desk/releases/tag/untagged-c32792b832d1967ae919
- Previous: **v0.2.8** (file copy/paste + Google Authenticator), built
  on top of v0.2.7 (Ask to connect). Test order: v0.2.6, then v0.2.8 (it
  contains v0.2.7). v0.2.8 draft:
  https://github.com/Bensalk/cipher-desk/releases/tag/untagged-16d8357fc1ec0eef551a
- The loopback test `a_resolution_change_resizes_the_viewer` once timed out on
  the Windows CI runner and passed on re-run (timing; passes locally).
- Previous: **v0.2.7** (Ask to connect). Bensal should test **v0.2.6
  first** (window + smooth picture):
  https://github.com/Bensalk/cipher-desk/releases/tag/untagged-2e87d6a9eaa470bd8946
  then v0.2.7 (give him its exact draft link; see "Release drafts").
- Previous build v0.2.5 (commit `0d05486`) finished: draft
  https://github.com/Bensalk/cipher-desk/releases/tag/untagged-944ba7ca4296b207802d
- Bensal tests on two Windows 11 Pro PCs (desktop + laptop) over Tailscale,
  from different networks. `tailscale ping` between them: about 28 ms.

## Done on this branch

| Version | What |
| --- | --- |
| v0.2.3 | Share screen keyboard/mouse fixed (SendInput needed `DESKTOP_JOURNALPLAYBACK`; input on its own thread). No "port in use" when the always-on service already shares. |
| v0.2.4 | Picture quality per connection (Best = lossless QOIZ, Balanced, Speed); host always sends the newest picture (less lag); view modes Original / Shrink / Stretch; real full screen (taskbar hidden); Cipher Desk window on the shared PC visible to the viewer (capture-hiding only while *viewing*). |
| v0.2.5 | Viewer holds a big change until the host has finished sending it (max 150 ms), so Best quality appears whole instead of in bands. `Session::poll` now returns `(events, caught_up)`. |
| v0.2.6 | Host window no longer hidden from capture at startup (startup race lost the "show" call). |
| v0.2.9 | **Folders and any file size**: shared `rdpv_core::xfer` now has `offer` (walks folders, folders before contents, max 20 000 items, links skipped), `Receiver` (one file at a time, 4 MiB `CHUNK` requests, `.part` file renamed when complete, removed on failure/drop, top-level " (1)" naming, every path component checked with `safe_file_name`) and `answer` (serves ≤ 8 MiB per request). Both the viewer (`session.rs` `receive_step`) and the host (`clipboard.rs`) use it. New `SessionEvent::FilesIncoming` → `UpdateKind::filesIncoming` ("Receiving 3 files, 1.2 GB…"); `FileReceived` is now one event per top-level item. Tests: folder tree + 10 MB file in pieces + empty file + name clash; unsafe names; failed piece cleanup. |
| v0.2.8 | **File copy/paste like AnyDesk**: host `clipboard.rs` sends files on a *new* CF_HDROP copy and puts a received batch on the clipboard (own-seq + "inside Received folder" checks stop echoes); viewer: C++ channel `rdpv/file_clipboard` (`app/windows/runner/file_clipboard.cpp`) + `session_screen.dart` `_offerCopiedFiles` / `_putReceivedOnClipboard`; `FileReceived` now carries the saved path (in `SessionUpdate.label`). **Two-step (TOTP)**: viewer sends device id + code as a Client Info password *cookie* (`rdpv_core::second_factor`, magic starts with a lone surrogate so it isn't text); vendored server `CookieCheck` runs after CredSSP, refuses with `ServerFreshCredentialsRequired` → `ConnectionError::CodeNeeded` → app asks for the code and reconnects. Host `rdpv_host::totp::Gate` (RFC 6238 tested; ±1 step; device remembered 30 days as sha256 hash; 5 wrong → 15 min lock). Service: `twostep-begin/finish/off`, status `TWOSTEP`; worker reads the secret at each sign-in. Bridge `start*` gained `share_screen`, `auth_code` (frb regenerated). Loopback test covers code needed / right code / remembered / other device. |
| v0.2.7 | **Ask to connect (no password)**: the always-on worker (`rust/host-service/src/worker.rs`) also listens on port+1 with public sign-in `ask` / `ask-to-connect`, `require_approval = true`; popup via `WTSSendMessageW` (Yes/No, 30 s timeout = refuse). Declines are sent as `ServerDeniedConnection` (vendored `server.rs`), shown as new `ConnectionError::AccessDeclined`, never auto-reconnected. App: ⋮ menu "Ask to connect" on Share screen connections (`home_screen.dart` `_ask`). Loopback tests cover allow and decline. |

## Waiting for Bensal's test results

1. **Window visibility (v0.2.6 fix):** host's Cipher Desk window was still
   invisible-but-clicking (photo confirmed, via the always-on service). Cause:
   `flutter_window.cpp` set `WDA_EXCLUDEFROMCAPTURE` at startup, and Dart's
   first `setSecure(false)` can run before the channel handler exists and is
   lost, so the host window stayed hidden. v0.2.6 no longer protects by default.
   Needs his confirmation on the **host** PC.
2. **Best quality smoothness (v0.2.5):** he says AnyDesk/TeamViewer look better.
   If still not good enough, next levers: faster screen grab (DXGI Desktop
   Duplication instead of the current capture), adaptive quality from measured
   bandwidth, frame markers so the viewer knows exactly when a frame is complete.

## Next features Bensal asked for (AnyDesk-like), in his priority order

1. **Accept / Reject popup — first part done in v0.2.7** (two ports = two ways
   in, since CredSSP checks one password per listener). Still to do:
   "always allow this device" checkbox; the popup also when sharing manually
   from the app (today only with always-on sharing); show who is asking.
2. **Unattended access password** remembered on his own viewer PC (he must never
   retype it). Saved password in the OS vault mostly covers this; make sure it
   works for the always-on service too.
3. **2FA with Google Authenticator** — done in v0.2.8 (always-on sharing
   only). v0.2.9 adds the QR code (`qrcode` crate, encoder only, in
   `rdpv_host::totp::qr_rows`; checked by decoding it with zxing-cpp; service
   reports it as `TWOSTEP_QR`, app draws it). Possible next: 2FA for manual
   sharing too.
4. **File transfer** — Bensal clarified he wants AnyDesk-style Ctrl+C/Ctrl+V
   of files: done in v0.2.8 (Windows; files up to 100 MB each, no folders,
   whole file in memory). v0.2.9: folders and any size (pieces of 4 MiB).
   Possible next: a progress bar, resuming a broken copy.
5. Backlog: auto-adapt the host resolution to the viewer window.

"Admin-level / full control" = the always-on Host service (runs as system,
reaches sign-in and lock screens). Already done; explain, don't rebuild.

## Sandbox gotchas (Claude's cloud workspace)

- No Flutter/Android SDK: CI is the compile check for Dart. Rust builds locally.
- `rust-toolchain.toml` pins 1.97.0 but rustup can't download; the installed
  `stable` *is* 1.97.0, so use `RUSTUP_TOOLCHAIN=stable cargo … --offline`.
- flutter_rust_bridge codegen: install the codegen binary with cargo and put
  small fake `flutter`/`dart` scripts on PATH (only version queries are made).
  Check the output is reproducible (hash before/after) before trusting it.
- The local clone may only fetch `main`, so `origin/connection-methods` can be
  stale. Check the real branch tip with
  `gh api repos/Bensalk/cipher-desk/branches/connection-methods --jq .commit.sha`.
- GitHub GraphQL is blocked: use `gh api` REST (`gh release list` / `gh run list` fail).
- Start a build: `gh api -X POST repos/Bensalk/cipher-desk/actions/workflows/release.yml/dispatches -f ref=connection-methods -F 'inputs[make_release]=true'`.
- After bumping versions, fix `rust/fuzz/Cargo.lock` too and verify both with
  `cargo metadata --locked --offline`.
- The Android emulator CI job sometimes times out (exit 124); re-run only the
  failed job before suspecting the code.

## Release drafts

Every "Run workflow" with `make_release` makes a new **untagged draft** with the
same version name, so several "v0.2.4" drafts existed and Bensal installed an
old one. Always bump the version for a new test build, and always send him the
exact draft link. Deleting old drafts needs his explicit OK.
