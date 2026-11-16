# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 16 Nov 2026._

## Current branch and build

- **Work is now on `main`** — the `connection-methods` branch was merged.
  All releases now dispatch from `main`.
- **v0.3.29 (16 Nov 2026) — waiting for Bensal's test.** Tray shows who is
  connected (reverse-DNS name + IP), warns on wrong passwords, Connection
  history (`connections.log`). Security fix C1 below. v0.3.27/28 confirmed:
  file copy both ways works, no duplicates.
- **Hardening to-do before v1.0 (from the 16 Nov 2026 review; keep details
  out of this public repo):** received-file names (done in v0.3.29); install the
  service program in an admin-only folder; lock down the service data folder
  and how its secrets are stored; ask-to-connect off by default and listen only
  on the Tailscale network; pre-login timeout and per-address lockout; verify
  both ends of the tray link; restrict who may control the service; stop the
  service reading the clipboard itself; size cap for received files.
- **v0.3.27 (16 Nov 2026) — waiting for Bensal's test.** Fix for host→viewer
  file copy + tray icon:
  - Clean v0.3.26 retest (other remote-access app closed) confirmed the cause: host copy
    logs `ole_formats=[none]`; viewer→host still works.
  - New `tray` command (`rust/host-service/src/tray.rs`), started by the service
    **as the signed-in user** (`launch::spawn_tray`, WTSQueryUserToken). Icon:
    teal online / amber connected / grey offline. Menu: Disconnect viewer, Go
    offline, Go online… (runs `go-online` elevated → service user control 130,
    or starts the service).
  - Worker ↔ tray link (`link.rs`): named pipe `\\.\pipe\CipherDeskHost-<session>`,
    SDDL SY + IU, same-session checks both ways. Tray polls every 400 ms; the
    worker asks for copied files (`want` id) only from the clipboard poller,
    i.e. only while a viewer is connected (Bensal's choice).
  - Worker reads/serves host files impersonating the signed-in user
    (`clipboard::read_files_as_signed_in_user`, `winclip::SignedInUser`) —
    also closes an old hole where SYSTEM could send files the user can't read.
  - Go offline = worker exits 75 → service keeps it off **in memory only**:
    back online after reboot (Bensal's choice: he needs remote access while
    away). Disconnect viewer = worker exits 76 → fresh worker.
  - Host logs reopen if the file is deleted (checked every 5 s). Tray logs to
    `tray.log` in the same folder if it may write there.
  - CI: Rust jobs pass. Flutter jobs fail on a **pre-existing** check
    ("frb_generated.rs is out of date", failing since before v0.3.24; releases
    still build). Worth fixing separately.
  - Bensal's test: install on both PCs, other remote-access apps closed. Tray icon on host?
    Copy a file in Explorer on the host → paste on viewer. Viewer → host still
    works? Try Disconnect viewer / Go offline / Go online. Send worker.log
    (+ tray.log if present).
- **v0.3.26 test result (15 Nov 2026, from host worker.log):**
  - **Viewer→host file copy WORKS** in Cipher Desk (log: "got file list from
    viewer" → 195 KB received → CF_HDROP written). Failed once in the first
    session after restart (no file list arrived at all; cause unknown).
  - **Host→viewer still does NOT work in Cipher Desk.** Every host copy logs
    `owner=explorer.exe; formats=[49161 "DataObject"]; ole_formats=[none]` and
    the host never offers files. Bensal's successful host→viewer pastes were
    almost certainly **another remote-access app's** clipboard sync (it was open;
    the log showed that app as the clipboard owner). Test with it closed.
  - **Root cause (host→viewer):** the worker runs as SYSTEM (launch.rs dups the
    service's own token into the user's session). "DataObject" is OLE's own
    marker; Explorer's real formats live in its COM object, which the SYSTEM
    worker cannot read (`ole_formats=[none]`). Fix: read the host's copied
    files from a helper running **as the signed-in user** (WTSQueryUserToken +
    CreateProcessAsUser, or the user-session app) and pass paths to the worker.
- **v0.3.26** — build dispatched from `main` (15 Nov 2026). Both PCs are
  Windows 11 Pro. v0.3.25 log: host OLE answered has_files=no for CF_HDROP
  (still only `49161 "DataObject"` from explorer.exe). Text now works both
  ways. v0.3.26:
  1. Viewer (`app/windows/runner/file_clipboard.cpp`) `ReadFiles()` falls back
     to `OleGetClipboard` → `GetData(CF_HDROP)` (links ole32.lib). The viewer
     had the same classic-only bug; the host log showed no "got file list from
     viewer", so viewer→host file copy never started.
  2. Host `ole_read_files()` also tries "Shell IDList Array" via OLE
     (`parse_shell_idlist`, shared with the classic path).
  3. Host `describe()` lists every OLE format: `ole_formats=[... tymed=N]`.
     If OLE still shows nothing useful on the host, suspect the worker runs as
     SYSTEM and cannot get data from the user's Explorer object — next step
     would be reading the clipboard from a helper running as the signed-in user.
- **v0.3.25** — build dispatched from `main` (15 Nov 2026).
  **Big correction:** Bensal's test is a **FILE** copy (a small PDF, Ctrl+C in
  File Explorer on host, Ctrl+V on viewer) — not text. v0.3.22–v0.3.24 chased
  text. v0.3.24 log: `owner=explorer.exe; formats=[49161 "DataObject"];
  ole_has_text=no` — Explorer used `OleSetClipboard`, leaving only "DataObject"
  on the classic clipboard (no CF_HDROP, no Shell IDList Array), so
  `read_files()` found nothing. v0.3.25: `winclip::read_files()` now falls back
  to `ole_read_files()` → `IDataObject::GetData(CF_HDROP)` → `parse_dropfiles`.
  `describe()` now logs `ole=ok, has_text=…, has_files=…` or `ole=unavailable`
  (unavailable would mean the SYSTEM worker can't reach Explorer's data object
  — next step would then be reading the clipboard from a user-session helper).
  **To test:** install on viewer PC and host PC, restart host PC, connect,
  copy a small file in File Explorer on host, paste in File Explorer on viewer.
  If it fails, send host `worker.log` and check the `ole=` part.
- **v0.3.24** — build dispatched from `main` (15 Nov 2026).
  Release link: https://github.com/Bensalk/cipher-desk/releases (draft; Bensal publishes).
  **Root cause confirmed from v0.3.23's `worker.log`** (this is the real one —
  v0.3.22's and v0.3.23's theories were both wrong, see below):
  every copy on the host leaves `owner=explorer.exe; formats=[49161 "DataObject"]`
  and NEVER CF_UNICODETEXT, at every poll rate including 15 ms. "DataObject" is
  OLE's private clipboard format — whatever copies text on Bensal's host PC uses
  `OleSetClipboard`, not the classic `SetClipboardData`. The classic Win32
  clipboard functions (`IsClipboardFormatAvailable`, `GetClipboardData`,
  `EnumClipboardFormats`) structurally cannot see text placed that way, at any
  speed — this was never a timing race. Fixed by also reading through OLE's
  `IDataObject` (`OleGetClipboard` → `QueryGetData`/`GetData` for
  CF_UNICODETEXT), which is how other remote-desktop tools read clipboards
  reliably. See `winclip::ole_has_text` / `winclip::ole_read_text` in
  `rust/host/src/clipboard.rs`. `winclip::describe()` now also logs
  `ole_has_text=yes/no` so the next log confirms whether this was the real fix.
  **To test:** install on both PCs, restart host PC, connect, copy text on
  host, paste on viewer. If it still fails, share host `worker.log` — look for
  `ole_has_text=` in the "no text on clipboard" line.
- **v0.3.23** — released, did NOT fix host→viewer text (wrong theory: timing,
  not a format-visibility problem). Still useful: the 15 ms watcher and
  `winclip::describe()` diagnostic logging stay in place.
- **v0.3.22** — released, did NOT fix host→viewer text (Bensal: "still same").
- **v0.3.21** — build dispatched (run 43703352686, 15 Nov 2026).
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
  1. Install the MSI on both Windows PCs.
  2. Turn CapsLock ON on the viewer PC before connecting.
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

## Active investigation: host→viewer FILE TRANSFER (not text)

**The bug Bensal cares about is FILE TRANSFER host→viewer** (Ctrl+C a file in
File Explorer on the host PC, Ctrl+V in File Explorer on the viewer PC). He
asked earlier to drop text clipboard work and focus on files. v0.3.22–v0.3.24
wrongly worked on text; do not go back to text unless he asks.


v0.3.22's theory ("IronRDP wipes the clipboard") was **wrong**: nothing in our
host or IronRDP writes to the Windows clipboard except `write_text`/`write_files`
(grep confirmed). Bensal's v0.3.22 `worker.log` shows every change leaves only
one registered format `[49161]` on the clipboard and never CF_UNICODETEXT —
so **another program on Bensal's host PC replaces the clipboard** within ~0.5 s
of a copy. The worker runs as SYSTEM in the user's session on `winsta0\default`,
so it sees the real user clipboard. v0.3.23 captures text within 15 ms and logs
the owning program + format names; the next `worker.log` should name the culprit.
If the fast watcher still misses it, next step: `AddClipboardFormatListener`
on a message-only window (event-driven, no polling gap).

The viewer PC has **no worker.log** — that file only exists on the HOST PC (which
runs the Windows host service). The viewer's diagnostics are in the **session log**
inside the Cipher Desk app: tap "Copy Diagnostic Log" while STILL CONNECTED to a
session. This is not worker.log — it is different.

## Sandbox gotcha: spurious draft v0.3.9 release

There is a stale draft release with ID 445340661 for v0.3.9 on GitHub — it was
created by a workflow that ran on old code. It should be deleted but the REST
API returns 403 from the cloud session. Use Chrome automation
(`mcp__claude-in-chrome__*`) on the GitHub Releases page to delete it manually.

## Done on this branch

| Version | What |
| --- | --- |
| v0.3.26 | Files: viewer reads copied files via OLE too; host also tries Shell IDList via OLE; OLE format list in host log (awaiting test) |
| v0.3.25 | HOST→VIEWER files: read CF_HDROP via OLE IDataObject when Explorer leaves only "DataObject" (awaiting test) |
| v0.3.24 | HOST→VIEWER text: read via OLE IDataObject too (text copied via OleSetClipboard was invisible to plain Win32 clipboard calls) (awaiting test) |
| v0.3.23 | HOST→VIEWER text: 15 ms text watcher + cache; clipboard owner/format names in host log (did not fix it — wrong theory) |
| v0.3.22 | Offer host files at connect (text part did not work) |
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
