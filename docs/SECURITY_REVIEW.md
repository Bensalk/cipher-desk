# Security Verification (Stage 9)

Each security requirement from the approved plan, with how it was verified. Status:
✅ verified here · 🟡 verified in part (rest needs a device or CI) · ⏳ belongs to a
later stage · ❗ open. Threats and accepted risks: `docs/THREAT_MODEL.md`.

## Findings from this stage

| # | Finding | Severity | Status |
|---|---------|----------|--------|
| R32 | The written threat model (SA-2) and the host-hardening checklist (SA-4) did not exist | Medium (process) | ✅ `docs/THREAT_MODEL.md`, `docs/HOST_SETUP.md` |
| R33 | A hostile server could announce a 65535 × 65535 desktop; the app would try to allocate ~17 GB and crash | Medium (denial of service) | ✅ sizes over 8K refused (`check_desktop_size`, unit test, fuzz target) |
| R34 | The pin file was read without a size limit (every other data file has one) | Low | ✅ 4 MB limit like the rest |
| R35 | Frame rectangles sent to the UI were not clamped like the pixels copied for them | Low (wrong drawing, no crash) | ✅ one clamped rectangle for both |
| R36 | The Reconnect-button rule existed only in Dart, separate from the Rust error catalogue | Low (could drift) | ✅ moved to Rust (`user_can_reconnect`) with an error-matrix test |
| R37 | Android: a session that finished connecting while the app was already in the background got no 5-minute limit | Medium | ✅ timer armed when the connection completes in the background |
| R38 | After an automatic reconnect, this device's clipboard was not offered again until it changed | Low | ✅ offered again after reconnecting |
| R39 | One way of noticing a vanished network (the 10 s limit during a server "reactivation") was not logged | Low (diagnostics) | ✅ logged; explained the 10 s / 30 s detection times |

No critical or high-severity issue is open.

## Requirements

### From the plan's security list (S1–S13)

| Req | Requirement | Status | Evidence |
|-----|-------------|--------|----------|
| S1 | NLA + TLS 1.2+ only, no fallback | ✅ | Live T8 (no-NLA host refused); `connector_config`; `tls.rs` (TLS 1.2/1.3 only) |
| S2 | Certificate pinning, warning on change | ✅ | Live T1, T4, L2; unit tests in `pins.rs`, reconnect rules |
| S3 | Drive/printer/USB/audio redirection off; clipboard opt-in | ✅ | `connector_config` (no such channels; audio off); live C3 |
| S4 | Passwords only in the OS vault; wiped; redacted from logs | 🟡 | Linux vault live (T11, vault tests); live T7 and L7 (no password in logs, three encodings); Windows vault in CI; Android Keystore needs a device |
| S5 | OS confirmation before a saved password is used | 🟡 | Windows Hello toggle with warning; Android BiometricPrompt; Linux keyring (KI-15). Needs devices |
| S6 | Clipboard limit, indicator, one direction, sensitive on Android | 🟡 | Live C1–C6 (engine); indicator and Android flag need Flutter/device |
| S7 | Idle disconnect | 🟡 | Engine timer (bridge `drive`); setting saved; UI needs Flutter |
| S8 | Signed releases, checksums, SBOM, `SECURITY.md` | 🟡 (Stage 10) | Release workflow: APK signature verified (never the debug key), Authenticode when a certificate is configured, `SHA256SUMS`, CycloneDX SBOM with local paths removed (SBOM tested here). Packages are built in CI only |
| S9 | Linux files private (0600) in the user's folders | ✅ | `fsutil::files_are_private`; files in the XDG data folder chosen by `path_provider` (`~/.local/share/...`) |
| S10 | Android `FLAG_SECURE` on by default | 🟡 | `MainActivity.kt`; needs a device |
| S11 | Android: no backup/transfer; minimal permissions | ✅ (code) | `allowBackup=false`, extraction rules; permissions: network, foreground service (special use), notifications |
| S12 | Signing key offline, separate from authentication | ✅ (code) | Signing only from environment or untracked `key.properties` (gitignored) |
| S13 | Unattended: dedicated non-admin account; S5 and S7 on by default | ✅ | `docs/HOST_SETUP.md` §3; confirmation and 30-minute idle are the defaults |

### Added by the security review (SA-1 – SA-10)

| Req | Requirement | Status | Evidence |
|-----|-------------|--------|----------|
| SA-1 | VPN with two-factor for outside access; no public 3389 | ✅ (documented) | `docs/HOST_SETUP.md` §6–7; `SECURITY.md` |
| SA-2 | Written STRIDE threat model | ✅ | `docs/THREAT_MODEL.md` (late: R32) |
| SA-3 | Fuzzing of what the server sends us, plus our parsers | ✅ | `rust/fuzz`: 6 targets, 10 minutes each here (≈1.05 billion inputs, no crashes) and weekly in CI |
| SA-4 | Host-hardening checklist | ✅ | `docs/HOST_SETUP.md` |
| SA-5 | Independent security review before the first public release | ❗ | Must be done by someone other than the author before release |
| SA-6 | Never accept any certificate | ✅ | `scripts/check-policy.sh` (pre-commit and CI); live T1 |
| SA-7 | Logs capped at info in release; `sspi` off; password-in-logs test | ✅ | `logging.rs`; live T7 (with control) and L7 |
| SA-8 | Never retry after an authentication failure | ✅ | Unit tests (reconnect rules, error matrix); live L5 |
| SA-9 | No secrets or personal data in the source | ✅ | gitleaks on every commit and full history (clean); noreply identity; `.gitignore`; path remapping (rule order fixed in Stage 10, R41), checked by `scripts/check-binaries.sh` on every release file and the SBOM |
| SA-10 | Portable mode rules | 🟡 (Stage 10) | `portable.rs` (unit tests): data next to the app only with `portable.txt` (Windows ZIP) or a `.data` folder (AppImage); passwords never there; warning when saving one; a pin mismatch still blocks. App UI needs Flutter |

### Findings from the Stage 1 spike (SF-1 – SF-6)

| Req | Finding | Status |
|-----|---------|--------|
| SF-1 | IronRDP's default accepts any certificate | ✅ own verifier with pinning; SA-6 check |
| SF-2 | `sspi` logs the password (hex) at debug level | ✅ worked around (SA-7). Re-confirmed in sspi 0.23.0 and still present in 0.23.1 (latest): the `encrypt_message` debug span records the credentials buffer before encryption. **Private report drafted for the owner's approval; not sent and not stored in this repository** |
| SF-3 | No connect timeout | ✅ timeouts on every step; keepalive (Stage 8) |
| SF-4 | Mid-session reactivation | ✅ handled (live T10) |
| SF-5 | No Android clipboard backend | ✅ Kotlin bridge (device test pending) |
| SF-6 | Windows Home cannot host RDP | ✅ error message and `docs/HOST_SETUP.md` §1 |

## Static analysis and audits

| Check | Result |
|-------|--------|
| clippy with `-D warnings` (project lints: no `unwrap`/`expect`/print/`dbg`) | ✅ clean |
| clippy `pedantic` (review only) | Only style notes in our code; all 24 integer-cast warnings are in generated FFI glue (list lengths), not reachable with real data |
| `unsafe` code | None in our crates (`forbid`); only the generated FFI module, explicitly allowed |
| cargo-deny (advisories, bans, licences, sources) | ✅ clean (RUSTSEC-2023-0071 `rsa` ignored with a reason: the app performs no RSA private-key operations; re-checked for sspi 0.23.0, the comment in `deny.toml` was out of date) |
| OSV-Scanner | CI only (could not be installed here) |
| CodeQL (Rust, Actions) | CI only |
| Licences | Rust: allow-list enforced by cargo-deny. Dart packages used: flutter_rust_bridge (MIT), path_provider, local_auth, file_selector (BSD-3-Clause); full list shown in the app's licence page. Android: androidx.biometric (Apache-2.0). Test/dev tools (not shipped): tree-sitter checker (MIT, Unlicense), libFuzzer (Apache-2.0 with LLVM exception) |
| Supply chain | `Cargo.lock` with `--locked`; actions pinned to commits; **Dart packages not yet locked** (KI-36) |
