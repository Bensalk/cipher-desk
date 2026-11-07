# Threat Model (STRIDE)

Required by SA-2. Checked against the code as it is now.
Every mitigation names the code or test that implements it. Anything not mitigated
is listed under **Accepted risks**.

## 1. What we protect

| Asset | Why it matters |
|-------|----------------|
| **A1** RDP passwords (typed or saved) | Give full access to the remote computer's account |
| **A2** Session content: screen, keystrokes, clipboard | May contain anything the user works on |
| **A3** Connection list and history (computer names, addresses, usernames) | Personal data; a map of the user's computers |
| **A4** Trusted certificate fingerprints (pins) | If changed silently, an attacker's computer could be trusted |
| **A5** The remote computer's account | Repeated wrong passwords can lock it |
| **A6** The user's device and the app's integrity | A crash or code execution caused by a hostile server |
| **A7** The source code and its history | Must not leak the owner's data (SA-9) |

## 2. Who might attack, and from where

| Attacker | Can |
|----------|-----|
| **T1** Someone on the network path (public Wi-Fi, compromised router, DNS spoofing) | Read, change, block or redirect traffic; answer instead of the real computer |
| **T2** A hostile or compromised remote computer | Send any RDP data, any desktop size, any clipboard text; end sessions |
| **T3** Another app on the same Android phone | Read the clipboard or screen if allowed; send intents to exported components |
| **T4** Someone with the unlocked phone or computer for a moment | Open the app, look at notifications and the recent-apps view |
| **T5** Someone who finds a lost or copied portable drive, a backup, or an exported file | Read files the app wrote |
| **T6** A malicious or compromised dependency, build tool or CI step | Change what is built |
| **T7** Anyone reading the public repository | Read every committed file and the full history |

Out of scope: malware already running as the same user on the desktop (it can read the
OS vault and log keystrokes regardless of this app), a rooted/jailbroken phone, and
attacks on the remote computer itself (see `docs/HOST_SETUP.md` for hardening it).

## 3. Trust boundaries

```
 [User] ──UI── [Flutter app] ──FFI── [Rust core] ──TLS 1.2+/NLA── network ── [Remote computer]
                    │                    │
         Android: Keystore,       OS vault (Windows Credential Manager,
         clipboard, notification  Linux Secret Service); app data files
```

B1 network · B2 remote computer · B3 OS (vault, clipboard, notifications, files) ·
B4 other apps on the device · B5 build and release pipeline · B6 public repository.

## 4. Threats and mitigations

### Spoofing

| # | Threat | Mitigation | Evidence |
|---|--------|-----------|----------|
| S1 | T1 impersonates the computer to steal the password | TLS with SHA-256 certificate pinning (trust on first use). A changed certificate blocks the connection **before** NLA, so no password is sent. The "accept any certificate" option is never used | `core/src/tls.rs`, `pins.rs`; live T1 and T4; `scripts/check-policy.sh` (SA-6) |
| S2 | T1 answers instead of the real computer **after a network drop** | Reconnect requires the same pinned certificate; an unpinned one is refused without prompting | Live **L2**; unit `changed_certificate_while_reconnecting_is_blocked`, `unpinned_certificate_while_reconnecting_never_prompts` |
| S3 | First connection to a new computer (no pin yet) | The user must compare the fingerprint; the dialog says nothing has been sent yet | `session_screen.dart` approval dialog; accepted risk R-1 |
| S4 | Downgrade to legacy RDP security without NLA/TLS | Only NLA (CredSSP) over TLS 1.2+ is accepted | Live T8 (xrdp without NLA refused) |
| S5 | An imported connections file pretends to be a trusted setup | Import validates every field, gives new ids, never imports passwords, turns clipboard **off** | `profiles.rs` `import`; unit tests |

### Tampering

| # | Threat | Mitigation | Evidence |
|---|--------|-----------|----------|
| T-1 | T1 changes traffic in transit | TLS (rustls, ring provider; TLS 1.2/1.3 only) | `tls.rs` |
| T-2 | T5 edits the pin file on a portable drive to trust a new server | A pin mismatch still blocks; a missing pin causes a visible first-connection prompt (SA-10) | `pins.rs`; live T4 |
| T-3 | Damaged or hostile app data files (connections, settings, pins) | Size limit 4 MB, strict parsing, invalid entries skipped, damaged files set aside not deleted, atomic writes with backup; an unreadable file is never overwritten | `fsutil.rs`, `profiles.rs`, `settings.rs`; `persistence.dart` (R29) |
| T-4 | T6 tampers with dependencies, build tools or releases | `Cargo.lock` with `--locked`; cargo-deny; OSV-Scanner; GitHub Actions pinned to commit hashes; CodeQL; the AppImage tool pinned by SHA-256; releases are drafts the owner reviews; `SHA256SUMS`, SBOM, signed APK (and Authenticode when configured) | `.github/workflows/*.yml`, `deny.toml`, `docs/RELEASING.md` |

### Repudiation

Not a significant concern for a single-user client. The app keeps a local history of
connection outcomes (no content, no passwords) for the user's own reference.

### Information disclosure

| # | Threat | Mitigation | Evidence |
|---|--------|-----------|----------|
| I1 | Password in logs (SF-2: `sspi` logs it hex-encoded at debug level) | `sspi`/`picky` logs always off; release builds capped at info | Live **T7** (with a control proving the check works) and **L7** (reconnect and wrong-password paths) |
| I2 | Password in files, exports, error messages or the command line | Only in the OS vault / Android Keystore; exports contain no passwords; error text is fixed; no CLI password flag | `vault.rs`, `PasswordVault.kt`, `profiles.rs` export tests, `status.rs` `every_error_has_safe_text` |
| I3 | Password left in memory | Wiped on drop (`SecretString`, zeroize). Kept during a session to allow reconnecting; copies inside libraries and, on Android, in Dart cannot be wiped at once | `secret.rs`; accepted risk R-4 |
| I4 | Screen visible to screenshots, screen recording, recent-apps (T3, T4) | Android `FLAG_SECURE` on by default | `MainActivity.kt` |
| I5 | Computer name visible on the lock screen (T4) | The session notification is generic and private | `SessionService.kt` |
| I6 | Clipboard leaks (T2 reads what you copy; T3 reads what the host copies) | Clipboard off by default per connection; text only; 1 MB limit; one-direction options; host text marked sensitive on Android; Android reads only in the foreground | `clipboard.rs`; live C1–C6 |
| I7 | Backups or device transfer copy app data (T5) | `allowBackup=false` and data-extraction rules exclude everything | `AndroidManifest.xml`, `data_extraction_rules.xml` |
| I8 | Other local users read app data on Linux | Files written with mode 0600 | `fsutil.rs` `files_are_private` |
| I9 | Owner's data in the public repository or release files (T7) | SA-9: gitleaks pre-commit and CI over full history, noreply identity, `.gitignore`, local paths removed from release builds and the SBOM and checked | `scripts/hooks/pre-commit`, `.gitleaks.toml`, `release-build.sh`, `check-binaries.sh`, `make-sbom.sh` |

### Denial of service

| # | Threat | Mitigation | Evidence |
|---|--------|-----------|----------|
| D1 | Account lockout through automatic retries (A5) | Never retry after a password or account error (SA-8); at most 3 network retries | Unit `wrong_password_while_reconnecting_stops_at_once`, `error_matrix`; live **L5** |
| D2 | T2 announces a huge desktop (e.g. 65535 × 65535 = 17 GB) to crash the app | Desktop sizes over 8K are refused | `session.rs` `check_desktop_size` (**R33**, found by this review) |
| D3 | T2 sends huge clipboard text | 1 MB limit; oversized text dropped with a notice | Live C5 |
| D4 | T2 sends malformed protocol data | IronRDP parsing (upstream fuzzed); errors end the session cleanly | SA-3 fuzzing of our own parsers (Stage 9) |
| D5 | Network vanishes silently; app hangs | Keepalive + `TCP_USER_TIMEOUT`; every network step has a timeout (SF-3) | Live **L3** (30.6 s) |
| D6 | Hostile import or data file exhausts memory | 4 MB file limit, 500 connections, 100 history entries | `fsutil.rs`, `profiles.rs` |

### Elevation of privilege

| # | Threat | Mitigation | Evidence |
|---|--------|-----------|----------|
| E1 | Memory-safety bug exploited by T2 | Rust; `unsafe_code` forbidden in our crates (only the generated FFI module and the Host's Windows capture/input module `rust/host/src/win.rs` are allowed it, each block with a safety comment) | `Cargo.toml` lints; Stage 9 unsafe audit |
| E2 | T2 reaches local files or devices | No drive, printer, USB or smart-card channel exists in the app; audio is off; only the clipboard channel is added, and only when sharing is on; the device's own name is not sent; server redirection is refused | `session.rs` `connector_config` |
| E3 | T3 sends intents to the app | Only the launcher activity is exported; the session service is not; the notification uses an immutable `PendingIntent` | `AndroidManifest.xml`, `SessionService.kt` |

## 5. Accepted risks

| # | Risk | Why accepted / what the user can do |
|---|------|-------------------------------------|
| R-1 | First connection trusts what the user approves | Inherent to trust-on-first-use; the dialog explains what to compare |
| R-2 | A compromised remote computer sees everything typed and shared | Use the clipboard only with computers you trust |
| R-3 | Same-user malware on the desktop can read the OS vault | Outside the app's control; don't save passwords on shared or untrusted computers |
| R-4 | Some password copies stay in memory until freed | Library and platform limitations (Dart strings, platform channels, the reconnect copy) |
| R-5 | IronRDP is younger than FreeRDP | Fuzzing, pinned versions, advisory scanning, independent review before release (SA-5) |
| R-6 | The app cannot make the remote computer secure | `docs/HOST_SETUP.md` (SA-4) |
| R-7 | Android accessibility malware or a rooted phone can read the screen | Platform limitation |
| R-8 | VNC has no encryption beyond the password check and no server identity, and passwords are limited to 8 characters | Only allowed on the own network or Tailscale; Share screen is recommended instead |
| R-9 | A computer left signed in for unattended Share screen can be used by anyone at it | The tutorial explains the trade-off; Private Remote Desktop keeps the screen locked |
| R-10 | The sign-in-screen service runs as LocalSystem (unavoidable: only the system account may attach to the Winlogon desktop) | Opt-in per computer with a UAC prompt; minimal supervisor + one worker; not yet tested on real Windows and must have the SA-5 independent review before it ships |

## 5a. Share screen (Cipher Desk Host) and VNC

| # | Threat | Mitigation | Evidence |
|---|--------|-----------|----------|
| H1 | Password guessing against a sharing computer | NLA (CredSSP/NTLM) with a generated 16-character password from the OS random generator, kept in the OS vault; 5 failures in 10 minutes refuse new connections for 15 minutes | `rust/host` `limiter.rs`, `generate_password`; loopback test |
| H2 | A fake Host in the middle | TLS with the Host's own certificate (ECDSA P-256, created on first use in the app's data folder); the viewer pins its fingerprint like any RDP computer | `identity.rs`, `tests/loopback.rs` |
| H3 | The viewer reads the sharing password off the shared screen | The Cipher Desk window is excluded from screen capture on Windows while sharing, whatever the screenshot setting | `flutter_window.cpp`, `secure_window.dart` |
| H4 | Sharing goes on without the user knowing | Sharing starts only from the Share page or the user's own auto-start setting; a banner shows while someone is viewing, with Stop | `share_screen.dart` |
| H5 | Someone at the computer sees the session | By design (that is the method's purpose); the method choice and tutorial say so | `docs/methods/SHARE-SCREEN.md` |
| H6 | The sign-in-screen service (LocalSystem) is abused to run code as system | Installed/removed only with a one-time UAC prompt; it launches one worker into the active console session and does nothing else privileged; its own password is DPAPI machine-encrypted; **needs on-device testing and the SA-5 review before release** | `rust/host-service` (`service.rs`, `launch.rs`, `config.rs`), R-10 |
| V1 | VNC traffic read or changed on the network | VNC is used only with loopback, private, link-local and Tailscale (100.64.0.0/10, fc00::/7) addresses; every resolved address is checked before connecting | `rust/core/src/vnc.rs` `is_private_address`; unit tests |
| V2 | A fake VNC server | VNC has no server identity; accepted on the own network, and Tailscale authenticates devices | R-8 |

## 6. Open items

- **SA-5:** an independent security review before the first public release (not done by
  the author of the code).
