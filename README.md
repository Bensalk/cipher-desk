# Secure RDP Viewer

A secure, open-source remote desktop (RDP) viewer for **Windows, Linux and Android**.

> **Status: in development — Stages 1–5 done (plan, foundation, user interface,
> connection engine, saved passwords).** Not ready for everyday use: clipboard and
> saved profiles come in Stages 6–7.
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
| `scripts/` | Git hooks, security policy checks, release build helper |
| `docs/` | Plan, progress record and design notes |

## Building

Requirements: Rust 1.97.0 (pinned in `rust-toolchain.toml`), Flutter stable,
and for Android the Android SDK + NDK with Java 17.

```sh
sh scripts/install-hooks.sh           # enable the secret-scanning pre-commit hook
cargo test --workspace                # Rust core tests
source scripts/start-test-hosts.sh    # optional: local RDP test hosts (Linux, bash)
cargo test -p rdpv-core --test live_host -- --ignored --test-threads=1
cargo install flutter_rust_bridge_codegen --version 2.13.0 --locked
cd app
flutter_rust_bridge_codegen generate --no-add-mod-to-lib
flutter pub get
flutter run -d linux                  # or: -d windows, or an Android device
```

## Security

Please read [`SECURITY.md`](SECURITY.md). Report vulnerabilities privately,
never in a public issue.

## Licence

[Apache-2.0](LICENSE). Third-party components are listed in [`NOTICE`](NOTICE).
