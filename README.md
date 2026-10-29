# Secure RDP Viewer

A secure, open-source remote desktop (RDP) viewer for **Windows, Linux and Android**.

> **Status: in development — Stages 1–6 done (plan, foundation, user interface,
> connection engine, saved passwords, clipboard).** Not ready for everyday use:
> connections are not yet saved to disk (Stage 7).
> See [`docs/PROGRESS.md`](docs/PROGRESS.md) for what is done and what is next.

## Goals

- Secure connections only: **NLA + TLS 1.2+**, server **certificate pinning**, no custom crypto.
- Optional **saved passwords**, stored only in the operating system's vault.
- Optional **two-way text clipboard**, off by default.
- Unattended access to your own PCs, with biometric/PIN unlock for saved passwords.
- One shared Rust core (built on [IronRDP](https://github.com/Devolutions/IronRDP))
  with a Flutter interface on every platform.

## Repository layout

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
| `docs/` | User guide, host setup, threat model, security review, releasing, progress record, plan |

## Building

Requirements: Rust 1.97.0 (pinned in `rust-toolchain.toml`), Flutter stable,
and for Android the Android SDK + NDK with Java 17.

```sh
sh scripts/install-hooks.sh           # enable the secret-scanning pre-commit hook
cargo test --workspace                # Rust core tests
source scripts/start-test-hosts.sh    # optional: local RDP test hosts (Linux, bash)
cargo test -p rdpv-core --test live_host -- --ignored --test-threads=1
cargo test -p rdpv-bridge --lib live_ -- --ignored --test-threads=1 --skip live_l3
bash rust/fuzz/run.sh 60              # optional: fuzz each parser for 60 s
cargo install flutter_rust_bridge_codegen --version 2.13.0 --locked
cd app
flutter_rust_bridge_codegen generate --no-add-mod-to-lib
flutter pub get
flutter run -d linux                  # or: -d windows, or an Android device
```

## Security

Please read [`SECURITY.md`](SECURITY.md). Report vulnerabilities privately,
never in a public issue.

- [`docs/USER_GUIDE.md`](docs/USER_GUIDE.md): install, use, portable copies, uninstall
- [`docs/HOST_SETUP.md`](docs/HOST_SETUP.md): set up the remote computer safely (start here)
- [`docs/RELEASING.md`](docs/RELEASING.md): building releases, and the checklist before going public
- [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md): threats, mitigations and accepted risks
- [`docs/SECURITY_REVIEW.md`](docs/SECURITY_REVIEW.md): how each security requirement was verified

## Licence

[Apache-2.0](LICENSE). Third-party components are listed in [`NOTICE`](NOTICE).
