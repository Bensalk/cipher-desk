# CLAUDE.md — Cipher Desk

Secure, open-source Remote Desktop (RDP) viewer by Bensal for Windows, Linux and Android. Public repo (since 30 Oct 2026): write docs and changelog for users, not about the development history. Teal / amber app theme (Material 3, seed colour teal).

## Repo layout

```
/
├── rust/core/                 ← shared Rust core (IronRDP + rustls): connection, pinning, input, vault, logging policy
├── rust/bridge/               ← flutter_rust_bridge API (crate rust_lib_cipher_desk; name must match the library for Cargokit)
├── rust/fuzz/                 ← fuzzers for our parsers (separate workspace)
├── app/                       ← Flutter app (lib/, android/, windows/, linux/, integration_test/)
├── packaging/                 ← .deb, AppImage, MSI (WiX v5), portable ZIP, signing
├── scripts/                   ← CI helpers, test hosts, policy checks, Windows build-on-this-pc.ps1
├── tools/                     ← test-only RDP host, icon generator, screenshot hero builder, syntax checker
├── docs/                      ← HOW-TO-USE, HOST_SETUP, THREAT_MODEL, SECURITY_REVIEW, RELEASING, screenshots
├── .github/workflows/         ← ci.yml, security.yml, release.yml, screenshots.yml
├── CHANGELOG.md, CONTRIBUTING.md, SECURITY.md, NOTICE, LICENSE (Apache-2.0)
└── README.md
```

## Build and release

- Every push/PR runs CI: Rust (Linux, Windows), live tests against local RDP test hosts, Flutter app on Linux/Windows/Android, an Android emulator with a live session, security scans. Docs-only changes skip CI.
- No Flutter or Android SDK in Claude's cloud sandbox: the CI run is the compile check. Job logs can't be downloaded there; failing steps print their errors as annotations (`scripts/ci-run.sh`), read with `gh api repos/Bensalk/cipher-desk/check-runs/<job>/annotations`.
- **Release:** bump the version in `app/pubspec.yaml` (`X.Y.Z+N`, raise N) **and** `Cargo.toml`, add a `## vX.Y.Z (date)` entry at the top of `CHANGELOG.md`, then push tag `vX.Y.Z`. `release.yml` builds and checks every package and creates a **draft pre-release**; the owner publishes it. "Run workflow" builds without publishing.
- Android signing: secrets `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. The owner keeps the keystore offline. **Never create or replace the signing key**: updates would no longer install over the app. Tag builds refuse a debug-signed APK.
- Windows files are unsigned until a code-signing certificate is added (`WINDOWS_CERT_*` secrets).
- Commit identity: `Bensal <Bensalk@users.noreply.github.com>`. Never a personal email.
- `rust/bridge/src/api/session/live_tests.rs` hardcodes exact numbers from
  `rust/core/src/policy.rs`'s defaults (e.g. `max_auto_reconnects`, the
  `(attempt, max)` pair in `live_l1_reconnects_after_the_connection_is_cut`).
  Changing a `SecurityDefaults` value needs a matching check here too, or
  the "Live RDP host tests" CI job fails with a stale assertion.

## Security rules (see CONTRIBUTING.md)

- No secrets or personal data in the repo; examples use `example.com` / `192.0.2.x`.
- NLA + TLS only; never accept any certificate; certificate pinning on first use.
- Passwords only in the OS vault (Credential Manager / Secret Service / Android Keystore), never in app files, exports, logs or command lines.
- `sspi`/`picky` logging always off (SF-2: sspi logs the password at debug level; Devolutions classed it as hardening, public PR welcome).
- Third-party actions pinned to full commit SHAs.

## App facts

- App ID `io.github.bensalk.cipherdesk`; Android minSdk 29 (Android 10+); program `cipher_desk`.
- Settings and connections in the app's data folder (`portable.txt` / `<AppImage>.data` for portable copies).
- MSI UpgradeCode `{E56EAB74-3E6E-4A89-948B-8E6CC77A3275}` must never change.

## Handoff

- Read `HANDOFF.md` first: current branch, newest test build, what's waiting for Bensal's test, next features, sandbox gotchas.
- Before a session ends (or usage runs low), update `HANDOFF.md` and commit it.
- Every new test build gets a new version number; send Bensal the exact release link.

## Release workflow gotchas

- **Never publish a release before the workflow runs.** The `release.yml` workflow skips all build jobs if the release already exists and is not a draft (`isDraft == false`). The correct flow: push tag → workflow creates a draft → builds attach → Bensal publishes. If a release is accidentally published early, delete it (keep the tag), then re-trigger via `gh api -X POST repos/Bensalk/cipher-desk/actions/workflows/release.yml/dispatches -f ref=vX.Y.Z`.
- **Tag push is blocked in cloud sessions** (HTTP 403 via GitHub proxy). Workaround: use Chrome automation on the GitHub new-release web UI to create the tag + release form, then trigger the workflow via `gh api` dispatch. Alternative: dispatch the workflow on the **branch** (`ref=connection-methods`) with `inputs[make_release]=true` — when `REF_TYPE=branch` the skip condition doesn't fire, so this avoids the tag-push block entirely.
- **`gh release view` (GraphQL) vs `gh api` REST discrepancy**: GraphQL may report a release as already published when REST returns 404. Always use `gh api repos/.../releases/tags/vX.Y.Z` to check; never trust the GraphQL result to skip a build.
- **`.claude/settings.json`** in the repo root has allow rules for auto-mode: `git push origin v*`, `git tag v*`, `gh release create *`, `gh workflow run *`. Safe to be public — no credentials.

## Android / Gradle version requirements

- Gradle wrapper (`app/android/gradle/wrapper/gradle-wrapper.properties`): must be **≥ 9.3.1**. Use `gradle-9.3.1-bin.zip` (not `-all`).
- Android Gradle Plugin (`app/android/settings.gradle.kts`, `com.android.application` version): must be **≥ 9.1.1** (Flutter minimum). Current: `9.1.1`.
- Bump both together when Flutter raises its minimum; they are separate files.

## flutter_rust_bridge (FRB v2.13.0) codegen

- After adding a new Rust API module under `rust/bridge/src/api/`, the FRB codegen **must** be re-run to generate the Dart bindings. `flutter_rust_bridge_codegen generate` cannot run in the cloud sandbox (network blocked). **Write the generated files manually** following the patterns in existing modules.
- For each new **sync** function in a new module (e.g. `log.rs`):
  1. Add `#[flutter_rust_bridge::frb(sync)]` above each `pub fn`.
  2. Create `app/lib/src/rust/api/<module>.dart` — thin wrappers calling `RustLib.instance.api.crateApi<Module><FnName>(...)`.
  3. Add abstract method declarations to `app/lib/src/rust/frb_generated.dart` (in the `abstract class RustLibApi` block).
  4. Add concrete `@override` implementations in `RustLibApiImpl` with the next available `funcId` (check the highest existing one with `grep funcId frb_generated.dart | tail`).
  5. Add `wire__crate__api__<module>__<fn>_impl` functions to `rust/bridge/src/frb_generated.rs` following the sync pattern.
  6. Add dispatch table entries (`funcId => wire__..._impl(...)`) in `rust/bridge/src/frb_generated.rs`.
- funcId values used: 1–53 (session/host/store/vault/info), 54–56 (log module: init_logging, session_log_export, session_log_clear). Next new module starts at 57.

## Working style

- Bensal installs from GitHub (APK on his phone, MSI/ZIP on Windows); he doesn't use an IDE. A broken build means no app.
- Keep explanations short and non-technical.
- Start every reply with a `Model:` line (Sonnet for routine/support, Opus for hard coding, design decisions, demanding work) and a one-line reason; Bensal can't judge difficulty himself.
- About screen: "Created by Bensal" is the last item, small and not highlighted.
