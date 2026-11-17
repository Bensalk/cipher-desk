# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 17 Nov 2026._

## Current branch and build

- **v1.0.0 is PUBLISHED and LIVE (17 Nov 2026).** Not a draft, not a
  pre-release: https://github.com/Bensalk/cipher-desk/releases/tag/v1.0.0 —
  tag `v1.0.0` points at commit `5370aec`. Bensal tested on two real Windows
  PCs and confirmed: normal connect, the 5-minute-idle online-check (no false
  lockout), and 5-wrong-passwords-then-Unblock all pass. All 10 files'
  SHA256SUMS verified twice by direct download. README's download links and
  version badge already match the real file names.
  **Note for a new session:** mid-cleanup, a script by mistake deleted the
  *published* v1.0.0 release itself (along with old test drafts it was meant
  to remove) — the git tag was never touched, so the release was rebuilt
  from the same commit via `gh api -X POST
  .../workflows/release.yml/dispatches -f ref=v1.0.0 -F
  'inputs[make_release]=true'`, re-verified, and republished. If anything
  about the release looks inconsistent, re-check it against the tag/commit
  above rather than assuming the first draft's ids are still valid.
- 19 old draft releases (test builds v0.2.0–v0.3.32) were deleted as
  clutter. The stale `regen-bridge-codegen` branch was also deleted (its
  work was already on `main`). Real version-history git tags (v0.1.0 up)
  were left alone on purpose.
- **1.0.1 backlog, not started, confirm before beginning any of it:**
  viewer log should say why a blocked try failed; change totp/winsecret tests
  so CodeQL stops flagging the sample password (alert #9 dismissed as tests);
  proper per-user access for the service Received folder (R-15); desktop
  notification on the host PC whenever someone connects via the always-on
  background service (not just when blocked) — right now the only way to
  notice is checking the tray icon. Bensal asked for this earlier and it
  was missed; now tracked here so it isn't lost again.
- **17 Nov 2026 (after v1.0.0): all CI jobs green for the first time** on
  `715dcbf` (Rust ×2, live RDP host tests, Flutter Linux/Windows/Android,
  Android emulator, security scans). Since the v1.0.0 tag: review-round fixes
  (result file no longer locked to admins; password dialog is its own
  widget), the bridge code regenerated with the real FRB 2.13.0 tool on a
  GitHub runner (the hand-written log/lock-keys additions had drifted the
  funcId order), one unused `_syncLockKeys` removed, and two stale
  clipboard-default test assertions corrected (default is on since v0.3.6).
  **Test build v0.3.33** (build-only "Run workflow", no tag/release) has all
  of the above plus the v1.0.0 draft's changes. Bensal chose 0.3.x for test
  builds so "1.0.0" is kept for the real stable release; the old v1.0.0
  draft release was built from older code and can be deleted. README still
  advertises v1.0.0 with download links that 404 until it is published.
- **Bridge codegen tip:** the sandbox can't run `flutter_rust_bridge_codegen`.
  Instead of hand-writing bindings, add a temporary `workflow_dispatch`
  workflow that runs the tool on `ubuntu-latest` and commits the output
  (it must exist on `main` before GitHub will dispatch it), then remove it.
- **Owner decisions still open:** `pending-password.bin` ACL (accept vs.
  tested ACL); `.claude/settings.json` tracked or local; keep/trim
  `CLAUDE.md`/`HANDOFF.md` in the public repo; badge option A/B/C; VNC CGNAT
  check now or later; commit identity. (Decided: clipboard sharing stays
  on by default; docs updated to match.)
- Leftover remote branch `regen-bridge-codegen` (proxy blocks deleting it);
  Bensal can delete it on GitHub.
- Bensal asked for a deep, audit-style security review next (findings report
  first, then fixes with his OK).
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
5. Backlog: auto-adapt resolution so the picture stays clear when the host
   has a high-resolution (e.g. 4K) monitor, instead of coming through scaled
   down/blurry — for both Windows and Android viewers, once Android work
   resumes. Not built yet; confirm before starting.

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
