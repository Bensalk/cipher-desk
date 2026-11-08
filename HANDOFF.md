# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 8 Nov 2026._

## Current branch and build

- Work happens on branch **`connection-methods`** (PR #12, open, not merged to
  `main`). It adds three connection methods: Private Remote Desktop, Share
  screen, VNC, plus the always-on Host service (sign-in screen / unattended).
- **v0.3.3** (commit `8051c80`, built and released as a draft, link below):
  two fixes, both from Bensal's live testing of v0.3.2 on 8 Nov.
  1. **More patient auto-reconnect.** `max_auto_reconnects` 3 → 6 and the
     backoff curve 2/5/10s → 2/5/10/15/20/30s (`rust/core/src/policy.rs`,
     `rust/core/src/status.rs`) — about 1.5 minutes of retrying instead of
     ~17 seconds. Bensal's "Connection lost" screenshots were over a
     Tailscale path that goes through a relay rather than a direct
     connection (confirmed earlier with `tailscale netcheck`: hard NAT,
     `MappingVariesByDestIP: true`); AnyDesk kept working the same moment
     Cipher Desk gave up, which is what pointed at "retries too briefly"
     rather than "can't reach the computer at all" (Check readiness also
     stayed green throughout). Caught and fixed a stale live-test
     assertion this broke (`live_l1_reconnects_after_the_connection_is_cut`
     expected "attempt 1 of 3") in the same build cycle — see commit
     `8051c80`.
  2. **Fixed a real file-copy-paste bug**, found from a screenshot of
     Windows' own "Error Copying File or Folder: The object invoked has
     disconnected from its clients." Root cause:
     `_putReceivedOnClipboard` in `session_screen.dart` called
     `FileClipboard.writeFiles` (a fresh `EmptyClipboard`+
     `SetClipboardData(CF_HDROP, ...)`) once per `FileReceived` event, and
     a folder/multi-file batch fires one event per top-level item — so a
     paste that started right as a second file landed could have the
     clipboard rewritten out from under Explorer mid-copy. Now debounces:
     waits 600ms after the last file in a batch before writing the
     clipboard once with the whole batch.
  **Still open:** Bensal has not yet confirmed either fix actually resolves
  what he saw (he was blocked testing when this was built — "Ask to
  connect" was the workaround while v0.3.3 built). Also surfaced but not
  yet investigated: his saved password for "Server PC" seems to have gone
  stale, most likely because he changed the Share screen host's password
  at some point and the viewer's saved copy wasn't updated to match — told
  him to re-enter/re-save it; not yet confirmed fixed.
  Draft: https://github.com/Bensalk/cipher-desk/releases/tag/untagged-8805f71aebfc9b15e7b2
  — sent to Bensal 8 Nov, not yet tested.
- **v0.3.2** (commit `17c3264`, built and released as a draft, link below):
  two things.
  1. Fixes the trap where enabling Settings → "Copy and paste for new
     connections" did nothing for a connection saved *before* that toggle was
     turned on. Root cause: that Settings toggle (`settings.clipboardDefault`)
     only seeds the default for brand-new connections; each
     `ConnectionProfile`'s own `clipboardMode` is a separate persisted field
     (defaults to `off`, `app/lib/state/profiles.dart`) and is never touched
     retroactively. Fix: the in-session "Copy and paste is off for this
     connection" notice now has a **"Turn on"** button (`session_screen.dart`)
     that sets this connection's own clipboard mode to Both and reconnects
     immediately — same mechanism as changing picture quality mid-session
     (`onClipboardModeChanged`, mirrors `onQualityChanged` in
     `home_screen.dart`). This is very likely why Bensal's file copy-paste
     still failed in v0.3.0 even after he "enabled copy and paste for new
     connections from settings" — he was reconnecting to an existing saved
     connection, not a new one. Also confirmed by his report that right-click
     on the receiver PC showed no Paste option at all — consistent with
     `receive_dir` being `None` (nothing ever reached the clipboard) rather
     than a wire-protocol bug. **Next: get Bensal's retest result — tap
     "Turn on" on the notice, then retry the file copy — before assuming
     there's a deeper bug in the file-transfer code itself.** If it still
     fails after that, the next place to look is
     `rust/core/src/session.rs`'s `ClipAction::FileList` handler (already
     traced and looks correct).
  2. Online/offline dot on each saved connection in the list (green =
     answers now, red = doesn't), checked every 15 s via the existing
     `checkReachable` (`home_screen.dart` `_ProfileTile`/`_ProfileTileState`).
  Draft: https://github.com/Bensalk/cipher-desk/releases/tag/untagged-d22b3d53c33083209760
  — sent to Bensal 8 Nov, not yet tested.
- **v0.3.1** (commit `f020ce1`): the keyboard-shortcuts toolbar and the
  full-screen view/exit button are no longer fixed on top of the remote
  screen (Bensal: "it block some of the screen... blocking me to close
  file sometimes"). Both are now a small round `_FloatingToggle` button you
  can drag anywhere; tap to open the panel (keys, or full-screen controls),
  tap again to close it. Not yet built as a release — do that together
  with v0.3.2 below.
- **v0.3.0** (commit `1122d0d`): host service DPI-aware (`win::make_dpi_aware`, fixes cut-off picture/wrong clicks at 125–150 % scale); clipboard setting renamed "Copy and paste (text and files)", on by default for new Share screen connections, snackbar when off; clipboard opens retried 20×25 ms + 150 ms settle after a change (host Rust + viewer C++/Dart) because Explorer holds the clipboard right after Ctrl+C. Release run 42463588907: all platforms built on the 4th Android retry (GitHub-side 504 downloads, not our code). Draft: https://github.com/Bensalk/cipher-desk/releases/tag/untagged-bb40b1733a7b6d7ba824 — sent to Bensal. Bensal's test 8 Nov (v0.2.9): file copy did NOT work either way, text did; lag was Tailscale relay (DERP, hard NAT `MappingVariesByDestIP: true`, no UPnP), parked by him. Bensal's test 8 Nov (v0.3.0): picture/stutter much improved; file copy-paste **still** didn't work even after enabling the global clipboard default from Settings — this is what v0.3.2 above fixes/targets.
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

1. **v0.3.3's two fixes** (longer auto-reconnect, file-copy clipboard
   batching) — not yet confirmed. He was blocked testing v0.3.2 when this
   was built (couldn't reconnect with the password method; "Ask to
   connect" worked as a stand-in).
2. **His saved password for "Server PC"** — likely stale after he changed
   the Share screen host's password; told him to re-enter and re-save it
   on the viewer side. Not yet confirmed whether that was actually the
   issue or whether it's fixed now.
3. **Picture/video quality** — he liked v0.3.0's improvement a lot but
   asked for more ("my laptop display is 15 inch... try to improve image
   quality"). Not started; next lever is likely a faster screen grab
   (DXGI Desktop Duplication) on the host side. Explicitly lower priority
   than the connection/copy-paste fixes, and he knows it's separate,
   later work.
4. **Floating full-screen/keys button opacity** (from v0.3.1): he finds it
   too dark/solid, wants a Settings slider from solid to fully
   transparent. Explicitly deferred to a later update by him, not now.

## Official release (v0.4.0), once v0.3.3 is confirmed

Bensal wants to publish an **official** GitHub release (not just a draft
test build) before his weekly usage resets, and has agreed in principle to
release even with the network-reconnect robustness still being tuned —
reasoning that anything is a big step up from the ancient **v0.1.0** still
shown on the repo's main branch and README. Plan, once he confirms v0.3.3
works (or accepts it as-is):
1. Merge `connection-methods` (PR #12) into `main`.
2. Update **README.md** — it's still entirely on v0.2.1: the version badge,
   the "v0.2.1 is a test release" note, and the whole download table
   (links point at `.../releases/download/v0.2.1/...`, which don't exist).
   Also refresh the feature list (no mention yet of file copy-paste, 2FA,
   Ask to connect, VNC, the floating buttons, or the online/offline dots).
3. Spot-check `docs/*.md` (HOW-TO-USE, methods/*, THREAT_MODEL,
   SECURITY_REVIEW) match current behavior — several were updated
   piecemeal during test builds, but no full pass has been done.
4. Tag and publish as **v0.4.0** (his choice of number, to signal the size
   of the jump from v0.1.0), not v0.3.3 — update `CHANGELOG.md` to match.
5. Only publish the actual GitHub release (make the draft non-draft)
   himself, or ask him to — same rule as every other release so far.

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
