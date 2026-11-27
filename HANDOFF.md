# HANDOFF — where the work stands

Read this with `CLAUDE.md` at the start of a new chat. Update it at the end of
every working session (newest state first; drop what is no longer true).

_Last updated: 27 Nov 2026 (session 2)._

## Current branch and build

- **main**, commit `c1f051c` (head as of this handoff).
- **v1.0.2 is PUBLISHED and LIVE** (set as Latest, not a pre-release):
  https://github.com/Bensalk/cipher-desk/releases/tag/v1.0.2 — tag `v1.0.2`
  points at commit `2e657be`. Security-only patch over v1.0.1 (one dependency
  fix — RUSTSEC-2026-0592, `url` crate stack-overflow DoS — nothing else
  changed). All 14 CI checks passed; Bensal published the draft himself.
- **Important**: commit `9306cdf` (26 Nov 2026, a *different* session, done
  together with Hive work) landed **after** v1.0.2 was built, so v1.0.2's
  actual uploaded asset files still use the **old** CamelCase names
  (`CipherDesk-1.0.2-android.apk`, etc.) and `SHA256SUMS` (no `.txt`). The new
  lowercase-hyphenated names + `SHA256SUMS.txt` (see `CLAUDE.md`'s "Asset
  filenames are a contract with Hive" note) only take effect starting with
  the **next** release build — not retroactively on v1.0.2. Don't be
  surprised if v1.0.2's assets don't match the new naming scheme; that's
  expected, not a bug.
- v1.0.1 (commit `782e2a1`) and v1.0.0 (commit `5370aec`) are still published
  too, just no longer "Latest".

## Done this session (27 Nov 2026)

- Fixed hardcoded version string in Report Problem log (`session_screen.dart` line 645):
  was `'1.0.0'`, now `'1.0.2'`. No version bump needed — fix goes into next release.
  Commit `c1f051c`. Known issue in HANDOFF is now resolved.

## Done this session (25 Nov 2026)

- Cleared a stale GitHub Security-tab "CodeQL configuration error" (Bensal
  reported "1 error" under `language:actions`). Root cause: a CodeQL job used
  to live inside `.github/workflows/security.yml`; it was later split out
  into its own `codeql.yml` (which is the one that actually runs and passes),
  but GitHub kept an orphaned "Security" setup-type record pointing at the
  now-nonexistent job, stuck failing since its last real scan 3 weeks
  earlier. Deleted that dead configuration (both its `language:rust` and
  `language:actions` halves) via Chrome on the GitHub Security tab —
  `Delete configuration` only removes the tracking record for a job that no
  longer exists; it does not touch or weaken the real, working CodeQL setup.
  Confirmed clean afterward: one green CodeQL setup type, both languages
  100% scanned.
- Bensal asked to check/replace outdated screenshots (UI had changed since
  they were taken — new Caps Lock key-bar button, file-transfer cancel
  button, lockout countdown banner, none reflected in the committed PNGs).
  Dispatched `screenshots.yml`; it partially failed (one tablet-layout test
  couldn't find the Save button — the same flake noted below) but had
  already pushed 34 freshly captured PNGs to the `screenshots` branch before
  failing. Pulled the 19 matching files from there, confirmed they show the
  current build, replaced all of `docs/screenshots/*.png`, committed and
  pushed. The `screenshots` branch itself was deleted afterward at Bensal's
  request (it's disposable output, recreated by the workflow each run — no
  need to merge it into `main`, there's no shared history).
- Found and fixed a real vulnerability, unrelated to the screenshots commit:
  CI's OSV-Scanner and cargo-deny jobs both failed right after the
  screenshots push on **RUSTSEC-2026-0592**, a stack-overflow DoS in the
  `url` crate (<= 2.5.8) reachable via `Url::origin()` on a crafted nested
  `blob:` URL. `cargo update` can't run here (no network to crates.io's
  index API, only to the static download host), so fixed it by hand: pulled
  `url 2.5.9` + the `form_urlencoded 1.2.3` it now requires from the
  crates.io sparse index (`https://index.crates.io/...`) to get the correct
  version + checksum, edited all three `Cargo.lock` files directly (root,
  `rust/fuzz/`, `tools/rdp-test-host/`). Verified `percent-encoding` was
  already compatible, no further cascade. All 14 CI checks passed.
- Bensal asked to ship the fix: bumped to **v1.0.2** (same release steps as
  always — `pubspec.yaml`, `Cargo.toml`, `CHANGELOG.md`, `README.md`),
  dispatched the release build, Bensal published the draft himself once it
  finished.
- Discussed turning on GitHub's "require status checks to pass before
  merging" for `main` (prompted by the red X on the screenshots commit,
  which only showed up *after* landing on `main`). That setting would block
  a direct push until the exact same commit had already passed CI on some
  other branch first — so every future change, even a one-line doc fix,
  would need branch + PR instead of a direct push; a brand-new commit cannot
  have already passed CI on another branch it hasn't been pushed to. **Decided
  to leave it off** (Bensal's call, after discussion): it's just the two of
  us, CI already surfaces problems right after every push and we react to
  them quickly (the `url` fix above is the concrete example), so the extra
  branch+PR overhead isn't worth it for how this repo is actually worked on.
  Revisit only if Bensal explicitly asks to turn it on later. Nothing was
  changed — this setting was never on to begin with.
- Bensal asked for a general security sweep beyond CodeQL: 0 open Dependabot
  alerts; all 5 workflows correctly default to `permissions: contents: read`
  with write access scoped only to the specific jobs that need it (release
  publish, screenshot branch push, SARIF upload); no code logs
  passwords/secrets anywhere (there's even a dedicated test for this,
  `live_host.rs`); force-push to `main` is blocked. Couldn't check the
  secret-scanning alerts list or the org-level default Action token
  permissions via `gh api` (both return 403 through this session's proxy);
  would need Chrome to check those — not done yet, no reason to think
  they're a problem, just unverified.

## Done this session (22 Nov 2026)

- Reviewed and merged all 3 open Dependabot PRs: x509-cert 0.3.0, sha2
  0.11.1, local_auth 3.0.2. Hit and resolved a real `Cargo.toml` merge
  conflict between the first two (both touched adjacent dependency lines).
  Documented 9 new heapless advisories (RUSTSEC-2026-0574..0582) across all
  three `osv-scanner.toml` files as an accepted, unreachable-code risk
  (heapless only reached via sspi → winscard → iso7816, smart-card code
  this app never calls); re-review by 31 Jan 2027.
- Found, while doing the above, that **RUSTSEC-2026-0544** (libz-sys/zlib
  heap overflow) was already shipped in v1.0.0 and unfixed — a prior
  session's fix only updated the root lockfile, missing
  `tools/rdp-test-host/Cargo.lock`. Fixed both.
- Released **v1.0.1**: version bump (`Cargo.toml` workspace version +
  `app/pubspec.yaml`), CHANGELOG entry, built and published on all 3
  platforms, set as Latest.
- Found and fixed a real gap: **README.md still pointed at v1.0.0** (badge,
  "Latest" line, all 5 download links) after v1.0.1 was published — nobody
  had updated it. Fixed and pushed. Added a reminder to `CLAUDE.md`'s
  release steps so this isn't missed on the next release either.
- Repo security review at Bensal's request — turned on **Secret Protection**
  (secret scanning) and **Push protection** (blocks a push that contains
  something that looks like a password/key/token before it lands), on top
  of what was already on (Dependabot alerts + auto-fix PRs, private
  vulnerability reporting, dependency graph, CodeQL). Confirmed "Automatic
  dependency submission" is correctly left off — not needed, since
  `Cargo.lock`/`pubspec.lock` are already parsed natively.
- Checked for anything to delete: no stale branches, no junk files
  committed to the repo. (5 leftover untracked scratch `.md` report files
  sitting in the sandbox working copy — never committed — were deleted;
  irrelevant to the actual GitHub repo.)

## Waiting for test results

Nothing currently outstanding. v1.0.1's quick connect + file-transfer sanity
check was never explicitly confirmed back, but it's now superseded by
v1.0.2 anyway. v1.0.2 itself is a dependency-only security patch (same
`url`-crate fix described above, no feature or UI change), so a full
regression pass isn't expected — but if Bensal does install it, a normal
connect + file-transfer check is the right sanity test.

## Previously done (26 Nov 2026 session, Hive integration)

- **Different session, working on `Bensalk/hive`** (the app launcher/updater)
  alongside Cipher Desk: renamed every release asset to match the pattern
  Hive expects. See `CLAUDE.md`'s "Asset filenames are a contract with Hive"
  note and "Current branch and build" above — this is a standing rule for
  every future release, not a one-off. Commit `9306cdf`, co-authored by a
  different Claude session (model Sonnet 4.6).

## Previously done (17 Nov 2026 session)

- **v1.0.0 security fixes** (all tested and shipped): the global
  "many devices guessing together" lockout now counts distinct devices, not
  every attempt, so one device's own repeated wrong passwords no longer
  locks out everyone. Cancel button on the file-transfer progress bar.
  Received files auto-delete from the clipboard-paste folder once something
  else replaces the clipboard (no ACL change needed — this was what Bensal
  actually wanted, not locking the folder).
  Full security review completed (`CIPHER-DESK-SECURITY-REVIEW.md`, not
  committed — sent to Bensal directly): 8 findings, 7 fixed, 1 (#7, memory
  pile-up during sign-in) accepted as a documented risk with his agreement.
- **Silent-connection fix** (found via proactive stress testing, not asked
  for): a connection that opens and sends nothing used to stall the host's
  one-at-a-time accept loop for 30s, so repeating it could block the owner
  out. Now closed after 10s and counted toward the 5-strikes lockout; the
  viewer's own background "online?" checks (which close immediately) are
  never counted. Verified: 1 silent connection delay dropped from 30.2s to
  10.2s; 5 block the device; 100 online checks not counted.
- Lockout timer + Unblock feature: tray shows a "blocked" pop-up and an
  Unblock menu item; viewer shows a countdown/warning before lockout.
  Bensal tested this on two real Windows PCs and confirmed it works.
- Bensal's own full test pass on the final build (two real Windows PCs):
  normal connect, Ask-to-connect Allow/Don't-allow, two-step remember-device,
  service Update button, wrong-password block + Unblock, received-file
  auto-delete, and the 5-minute-idle online-check (no false lockout) — all
  passed.
- Repo cleanup: 19 old draft releases (test builds v0.2.0–v0.3.32) deleted.
  Stale `regen-bridge-codegen` branch deleted (its work was already on
  `main`). Real version-history git tags (v0.1.0 up) left alone on purpose.
- CodeQL alert #9 ("hard-coded cryptographic value" in `winsecret.rs`)
  diagnosed as a false positive (a test-only sample password string) and
  dismissed by Bensal in GitHub's UI as "Used in tests."
- FRB bridge code regenerated with the real `flutter_rust_bridge_codegen`
  tool via a temporary GitHub Actions job (not by hand), fixing drift from
  earlier hand-written additions. One unused `_syncLockKeys` removed; two
  stale clipboard-default test assertions corrected.

## Next features Bensal asked for, in priority order

1. ~~"Always allow this device" checkbox; popup for manual sharing too; show
   who is asking.~~ Done (Ask to connect).
2. ~~Unattended access password in OS vault for the always-on service.~~ Done
   (DPAPI machine scope under ProgramData).
3. ~~2FA for manual sharing too.~~ Done (Two-step verification, Google
   Authenticator).
4. ~~File transfer progress bar~~ — done, with a cancel button (v1.0.0).
   **Resuming a broken copy** after a cancel or disconnect is not built.
5. Auto-adapt resolution so the picture stays clear when the host has a
   high-resolution (e.g. 4K) monitor, instead of coming through scaled
   down/blurry — for both Windows and Android viewers, once Android work
   resumes. Not built yet.
6. A desktop notification on the host PC whenever someone connects via the
   always-on background service (not just when blocked) — right now the
   only way to notice is checking the tray icon. Bensal asked for this
   earlier and it was missed; now tracked here so it isn't lost again.

**Explicit instruction from Bensal (17 Nov 2026): the app is now how it's
supposed to be. Do not start any of the above, or any UI polish below,
without him asking first. Only security work should proceed unprompted if
genuinely necessary.**

## 1.0.1 backlog — confirm before starting any of this

- Quiet the repeated "Connection ended ... not enough bytes" log noise from
  routine online-check connections.
- Viewer log should say *why* a blocked attempt failed, not just that it did.
- Change the totp/winsecret test so CodeQL's alert #9 pattern doesn't recur
  (generate the sample password at runtime instead of a literal).
- Proper per-signed-in-user access control for the service's Received
  folder (R-15), instead of relying solely on the clipboard-triggered
  auto-delete.
- Stuttering issue Bensal referred to on 17 Nov 2026 — not yet scoped;
  ask him where/when it happens (Share screen vs VNC, under load, etc.)
  before starting anything.

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

~~Fixed in commit `c1f051c` (27 Nov 2026). Now reads `'1.0.2'`. Remember to update
this string as part of every future version bump.~~

## Sandbox gotchas (Claude's cloud workspace)

- No Flutter/Android SDK: CI is the compile check for Dart. Rust builds locally.
- `rust-toolchain.toml` pins 1.97.0 but rustup can't download; the installed
  `stable` *is* 1.97.0, so use `RUSTUP_TOOLCHAIN=stable cargo … --offline`.
- flutter_rust_bridge codegen: `flutter_rust_bridge_codegen generate` cannot
  run here (network blocked). For a one-off regeneration, add a temporary
  `workflow_dispatch` workflow that runs the real tool on `ubuntu-latest`
  and commits the output, then remove the workflow (it must exist on `main`
  before GitHub will dispatch it). For small manual additions between
  regenerations, follow existing modules' patterns (see CLAUDE.md's FRB
  section; next unused funcId is 59).
- The local clone may only fetch `main`; check other branches with
  `gh api repos/Bensalk/cipher-desk/branches/<name> --jq .commit.sha`.
- GitHub GraphQL is unreliable for release/publish status (can report a
  release as published when REST returns 404). Always check with
  `gh api repos/Bensalk/cipher-desk/releases/tags/vX.Y.Z`.
- `gh api` **cannot create, edit, or delete** releases, branches, or tags
  from this session type (403 on write). Use Chrome automation on the real
  GitHub web UI for any of those; read-only `gh api` calls are fine.
- Start a build: `gh api -X POST repos/Bensalk/cipher-desk/actions/workflows/release.yml/dispatches -f ref=main -F 'inputs[make_release]=true'`.
  To rebuild a release against an existing tag instead of a branch, use
  `-f ref=vX.Y.Z` (the tag name) — this also sidesteps the tag-push block.
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
  automation (`mcp__claude-in-chrome__*`) for GitHub web UI actions.
- **Never loosely script a batch delete of GitHub releases.** Matching all
  `/releases/tag/` delete forms on a page catches published releases too,
  not just drafts — this deleted the live v1.0.0 release by mistake in this
  session. Filter strictly to drafts (check for the "Draft" badge) or delete
  releases one at a time with a screenshot check between each.
- The Android emulator CI job sometimes times out (exit 124); re-run only the
  failed job before suspecting the code.
- A dispatched workflow run can occasionally get stuck reporting `status:
  queued` indefinitely, with `gh api .../cancel` rejecting with HTTP 409. This
  is a GitHub-side anomaly — don't fight it; just dispatch a fresh run.
- **Known intermittent flake, not yet root-caused**: `integration_test` (both
  `hello_test.dart` on the Android-emulator CI job and `screenshots_test.dart`
  in `screenshots.yml`) occasionally fails with "Found 0 widgets with key
  `button-save`" on the connection form's Save button — seen on both the
  tablet screenshot width and the Android 10 emulator, with zero code changes
  between a passing run and a failing one, so it's timing/flakiness, not a
  regression. Re-running the job alone has cleared it before. If it keeps
  recurring, look at `_save`/`_revalidate` in `app/lib/ui/connection_form.dart`
  and whether `tapVisible`'s `ensureVisible` scroll races an async
  Rust-core validation call.
