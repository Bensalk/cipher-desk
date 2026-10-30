<h1 align="center">
  <a href="https://github.com/Bensalk/cipher-desk"><img src="docs/logo.svg" alt="Cipher Desk" width="140"></a>
  <br>
  Cipher Desk
  <br>
</h1>

<h4 align="center">A secure, open-source Remote Desktop (RDP) viewer for Windows, Linux and Android.</h4>

<p align="center">
  <a href="https://github.com/Bensalk/cipher-desk/releases"><img src="https://img.shields.io/badge/release-v0.1.0%20test-FFC107?style=flat-square" alt="Release v0.1.0 (test)"></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20Android%2010%2B-00796B?style=flat-square" alt="Windows, Linux, Android 10+">
  <img src="https://img.shields.io/badge/Rust-1.97-B7410E?style=flat-square&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/Flutter-stable-02569B?style=flat-square&logo=flutter&logoColor=white" alt="Flutter">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-004D40?style=flat-square" alt="Apache-2.0"></a>
  <a href="https://github.com/Bensalk/cipher-desk/actions"><img src="https://img.shields.io/badge/build-GitHub%20Actions-2D6BFF?style=flat-square" alt="Built on GitHub Actions"></a>
</p>

<p align="center">
  <a href="#key-features">Key Features</a> •
  <a href="#screenshots">Screenshots</a> •
  <a href="#how-to-use">How To Use</a> •
  <a href="#download">Download</a> •
  <a href="#building">Building</a> •
  <a href="CHANGELOG.md">Changelog</a> •
  <a href="#privacy--security">Privacy &amp; Security</a> •
  <a href="#related">Related</a> •
  <a href="#license">License</a>
</p>

<p align="center"><img src="docs/hero.png" alt="Cipher Desk on a desktop and a phone"></p>

Cipher Desk connects to your own Windows computers over Remote Desktop, from a PC, a Linux machine or an Android phone or tablet. Made by **Bensal** with security first: it only makes protected connections, checks the computer's identity, and never stores a password anywhere except your device's own secure storage. New to Cipher Desk? Follow the [step-by-step guide](docs/HOW-TO-USE.md).

> [!NOTE]
> **v0.1.0 is a test release.** It passes all automated tests, including live connections to test computers, but it has not yet had an independent security review. Progress is recorded in [docs/PROGRESS.md](docs/PROGRESS.md).

## Key Features

- **Secure connections only** – Network Level Authentication (NLA) and TLS 1.2 / 1.3. Older, unprotected connections are refused, not downgraded.
- **Certificate check and pinning** – the first time, you compare the computer's fingerprint before anything is sent. If it ever changes, the connection is blocked before your password leaves the device.
- **Saved passwords, safely** – optional, and only in Windows Credential Manager, the Linux keyring or the Android Keystore, confirmed with Windows Hello or your screen lock. Never in the app's files, exports or logs.
- **Clipboard sharing** – two-way text clipboard, off by default; allow it per connection, in one direction or both.
- **Works on touch screens** – tap to click, long-press to right-click, two-finger scroll, trackpad mode, pinch to zoom up to 4×, and a key bar with Ctrl, Alt and Shift that stay held (for Ctrl+C or Alt+Tab), Ctrl+Alt+Del, Windows key, Esc, Tab, arrows, F1–F12 and *Type text*.
- **Saved connections** – search, Wake-on-LAN, a *Check readiness* test, connection history, and export / import without passwords.
- **Reliable** – reconnects by itself after a network drop (same computer, same certificate) and never retries a wrong password, so accounts don't get locked.
- **Clear errors** – every message says what happened and what to do next; *Settings → What error messages mean* lists them all.
- **Idle protection** – sessions end after 30 minutes without input (adjustable); on Android, 5 minutes after you leave the app.
- **Portable** – a Windows ZIP and a Linux AppImage that keep everything on a USB drive, except passwords.
- **Light and dark themes**, a side rail on tablets and desktops, and a bottom bar on phones.

## Screenshots

**Phone**

| Connections | New connection | Check the certificate | Remote session |
|:---:|:---:|:---:|:---:|
| <img src="docs/screenshots/phone-connections.png" width="230"> | <img src="docs/screenshots/phone-new-connection.png" width="230"> | <img src="docs/screenshots/phone-certificate.png" width="230"> | <img src="docs/screenshots/phone-session.png" width="230"> |
| **Settings** | **Clear errors** | **Error help** | **Dark theme** |
| <img src="docs/screenshots/phone-settings.png" width="230"> | <img src="docs/screenshots/phone-wrong-password.png" width="230"> | <img src="docs/screenshots/phone-error-help.png" width="230"> | <img src="docs/screenshots/phone-connections-dark.png" width="230"> |

**Desktop** (Windows and Linux)

| Connected to a computer | Readiness check |
|:---:|:---:|
| <img src="docs/screenshots/desktop-session.png" width="420"> | <img src="docs/screenshots/desktop-readiness.png" width="420"> |
| **Connections** | **Trusted computers** |
| <img src="docs/screenshots/desktop-connections.png" width="420"> | <img src="docs/screenshots/desktop-trusted-computers.png" width="420"> |

**Tablet**

| Connections | Settings |
|:---:|:---:|
| <img src="docs/screenshots/tablet-connections.png" width="300"> | <img src="docs/screenshots/tablet-settings.png" width="300"> |

> Screenshots are taken automatically from the real app by the [Screenshots workflow](.github/workflows/screenshots.yml), on Linux at phone, tablet and desktop sizes. The remote desktop shown is the project's local test computer. Only sample names and documentation addresses (`example.com`, `192.0.2.x`) are used. Run it again from the Actions tab after UI changes.

## How To Use

> 📖 **Full guide:** [docs/HOW-TO-USE.md](docs/HOW-TO-USE.md) covers installing, adding a computer, checking the certificate, sessions, settings, portable copies, uninstalling and troubleshooting.

**Quick start**

1. Set up the computer you want to reach: turn on Remote Desktop with Network Level Authentication and note its certificate fingerprint. See [docs/HOST_SETUP.md](docs/HOST_SETUP.md).
2. Install Cipher Desk and tap **New connection**. Enter the computer's name or IP address and your username.
3. Tap the connection and type your password (or tick **Save password** to keep it in this device's secure storage).
4. The first time, **compare the certificate fingerprint** with the one on the computer. Only trust it if they match.
5. Use the key bar for Ctrl+Alt+Del, the Windows key and F-keys. Tap the link icon to disconnect.

> [!TIP]
> Only connect over your own network or a VPN. Never expose Remote Desktop (port 3389) directly to the internet.

## Download

Latest: **v0.1.0 (test release)** – all files and checksums are on the [Releases page](https://github.com/Bensalk/cipher-desk/releases). Release notes are in [CHANGELOG.md](CHANGELOG.md).

| System | Download | |
|---|---|---|
| **Android 10+** | [CipherDesk-0.1.0-android.apk](https://github.com/Bensalk/cipher-desk/releases/download/v0.1.0/CipherDesk-0.1.0-android.apk) | Allow installing from your browser or file manager when asked |
| **Windows 10/11** (installer) | [CipherDesk-0.1.0-x64.msi](https://github.com/Bensalk/cipher-desk/releases/download/v0.1.0/CipherDesk-0.1.0-x64.msi) | Installs for your user only; no administrator rights needed |
| **Windows** (portable) | [CipherDesk-0.1.0-windows-x64-portable.zip](https://github.com/Bensalk/cipher-desk/releases/download/v0.1.0/CipherDesk-0.1.0-windows-x64-portable.zip) | Unzip anywhere and run `cipher_desk.exe` |
| **Debian / Ubuntu** | [cipher-desk_0.1.0_amd64.deb](https://github.com/Bensalk/cipher-desk/releases/download/v0.1.0/cipher-desk_0.1.0_amd64.deb) | `sudo apt install ./cipher-desk_0.1.0_amd64.deb` |
| **Other Linux** | [Cipher_Desk-0.1.0-x86_64.AppImage](https://github.com/Bensalk/cipher-desk/releases/download/v0.1.0/Cipher_Desk-0.1.0-x86_64.AppImage) | Make it executable and run it |

- Check a download with `SHA256SUMS` from the same release (`sha256sum -c SHA256SUMS`, or `Get-FileHash` on Windows).
- The Windows files are not code-signed yet, so Windows SmartScreen may warn you: choose *More info → Run anyway* after checking the checksum.
- Updates install over the existing app and keep your connections.

## Building

Every push is built and tested on GitHub Actions (Windows, Linux and Android), including live tests against local RDP test computers, fuzzing and security scans.

```bash
# Requirements: Rust 1.97.0 (pinned in rust-toolchain.toml), Flutter stable,
# and for Android the Android SDK + NDK with Java 17
$ sh scripts/install-hooks.sh                # secret-scanning pre-commit hook
$ cargo test --workspace                     # Rust core tests
$ cargo install flutter_rust_bridge_codegen --version 2.13.0 --locked
$ cd app
$ flutter_rust_bridge_codegen generate --no-add-mod-to-lib
$ flutter pub get
$ flutter run -d linux                       # or: -d windows, or an Android device
```

- Shared Rust core: `rust/core` (built on [IronRDP](https://github.com/Devolutions/IronRDP) and rustls); bridge to Flutter: `rust/bridge`.
- Flutter app for every platform: `app/`. Installers and portable packages: `packaging/`.
- Live tests, fuzzing and release builds: see [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/RELEASING.md](docs/RELEASING.md).

<details>
<summary>Repository layout</summary>

| Path | Contents |
|------|----------|
| `rust/core` | Shared Rust core: connection engine (TLS + NLA, certificate pinning, input, reactivation), security and logging policy, secrets handling, form validation, status and error messages |
| `rust/bridge` | flutter_rust_bridge API layer used by the Flutter UI |
| `app/` | Flutter app (Windows, Linux, Android) |
| `app/rust_builder` | Builds the Rust library for each platform (Cargokit, from flutter_rust_bridge) |
| `scripts/` | Git hooks, security policy checks, test hosts, release build helper |
| `rust/fuzz` | Fuzzers for our parsers of untrusted input (test-only, separate workspace) |
| `tools/rdp-test-host` | Test-only RDP host (NLA + scriptable clipboard) for live tests; never shipped |
| `tools/icon` | Draws the app icon (original artwork) for every platform |
| `tools/syntax-check` | Syntax-only Dart/Kotlin checker for machines without Flutter (optional) |
| `packaging/` | Installers and portable packages: Linux `.deb`/AppImage, Windows MSI (WiX)/portable ZIP, icons |
| `docs/` | How-to-use guide, host setup, threat model, security review, releasing, progress record, plan |

</details>

## Privacy & Security

- **No accounts, no tracking.** Cipher Desk has no analytics, telemetry or ads, and there is no Cipher Desk server: it talks to your computers directly.
- **Passwords stay on your device.** Saved passwords live only in the operating system's secure storage. They are never written to the app's files, never exported, and never logged.
- **Identity checked every time.** Each computer's certificate is pinned after you approve it; a changed certificate blocks the connection.
- **Clipboard off by default**, and screenshots of the app blocked on Android by default.
- **Android permissions, and why:**

  | Permission | Why |
  |---|---|
  | Internet | Connects to your computers |
  | Notifications / foreground service | Keeps a session open while you switch apps, and shows that a session is open |

  Windows and Linux need no special permissions; Windows Hello is used only to confirm a saved password.
- **Only install from this repo's [Releases](https://github.com/Bensalk/cipher-desk/releases).** Each release lists the SHA-256 of every file and the Android signing certificate, so you can check your download. The official Android signing certificate SHA-256 is:
  `B3:30:E4:86:BD:9A:5D:4C:D7:C3:1B:BF:72:54:E2:B3:09:DD:8C:05:23:81:07:42:79:E4:E9:3B:91:D4:14:CE`
  Android refuses updates signed with any other key, so a fake APK can't replace the real app.
- How it was checked: [threat model](docs/THREAT_MODEL.md) and [security review](docs/SECURITY_REVIEW.md). Setting up the remote computer safely: [docs/HOST_SETUP.md](docs/HOST_SETUP.md).
- Found a security problem? Report it privately, see [SECURITY.md](SECURITY.md).

## Related

- [Crosshair](https://github.com/Bensalk/crosshair) – Bensal's light crosshair overlay for FPS games on Android.
- [GST Calculator](https://github.com/Bensalk/gst-calculator) – Bensal's offline GST calculator for Android and Windows.

## License

Open source under the [Apache-2.0](LICENSE) licence. Third-party components are listed in [NOTICE](NOTICE).
