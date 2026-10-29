# Contributing & Coding Standards

## Setup

```sh
# Enable the repository's git hooks (secret scan before every commit)
sh scripts/install-hooks.sh
```

Requirements: Rust (version pinned in `rust-toolchain.toml`), Flutter stable,
and for Android builds the Android SDK + NDK. `gitleaks` must be installed for
the pre-commit hook.

Optional, for machines without Flutter: `cd tools/dart-syntax && npm ci` installs a
syntax-only Dart checker that the pre-commit hook then runs on staged Dart files. It finds
bracket and similar mistakes; it does not replace `flutter analyze`.

## Golden rules (security)

1. **No secrets or personal data in the repository** — no passwords,
   usernames, real hostnames, IP addresses, emails, keys or certificates.
   - Tests generate credentials at run time or read them from environment
     variables / CI secrets.
   - Examples use reserved placeholders only: `192.0.2.0/24`, `example.com`,
     `user@example.com`.
2. **Never use `DangerouslyAcceptInvalidCertificate`** or any "accept any
   certificate" code path. CI fails if it appears.
3. **Never log secrets.** Wrap passwords in `rdpv_core::secret::SecretString`;
   its `Debug`/`Display` output is always `***` and memory is wiped on drop.
4. **Never pass passwords on a command line.**
5. **No custom cryptography.**
6. New dependencies must pass `cargo deny check` (licences + advisories).

## Rust

- `cargo fmt --all` — formatting is enforced in CI.
- `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings.
- `#![forbid(unsafe_code)]` in our crates unless a reviewed exception is
  documented.
- Public functions return `Result` with typed errors; no `unwrap()`/`expect()`
  outside tests.
- Every new behaviour gets a unit or integration test.

## Dart / Flutter

- `dart format .` and `flutter analyze` with no issues.
- UI shows user-friendly messages; it never displays raw error text that may
  contain sensitive data.

## Commits

- Use your GitHub **noreply** email
  (`git config user.email <username>@users.noreply.github.com`).
- Small, reviewable commits with a clear message.
- Do not commit generated build output (see `.gitignore`).

## Definition of done (per stage)

Implementation, relevant tests, error handling and acceptance criteria are
checked; CI is green; results are recorded in `docs/PROGRESS.md`. Tests that
could not be run are reported as **not run**, never as passed.
