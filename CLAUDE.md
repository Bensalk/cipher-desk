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

## Working style

- Bensal installs from GitHub (APK on his phone, MSI/ZIP on Windows); he doesn't use an IDE. A broken build means no app.
- Keep explanations short and non-technical.
- About screen: "Created by Bensal" is the last item, small and not highlighted.
