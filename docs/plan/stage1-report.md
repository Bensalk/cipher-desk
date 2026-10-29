# Secure Open-Source Remote Desktop Viewer — Stage 1 Report & Final Plan

**Stage:** 1 — Research and Planning (approved 29 Oct 2026; updated with installers, portable versions and source-code privacy rules)
**Date:** 29 October 2026
**Platforms:** Windows, Linux, Android (all as viewers / clients)
**Next step:** Stage 2 (Project Foundation) — local checks done; awaiting first GitHub CI run.

---

## 1. Summary

- **Recommended stack:** Rust core on **IronRDP** + **Flutter** UI, connected with **flutter_rust_bridge**, plus small **Kotlin** parts for Android.
- **Proof-of-concept result:** the IronRDP engine passed the core checks on Linux: NLA login, TLS 1.3, certificate pinning, clear error messages, a connect timeout, and **two-way text clipboard** (with clipboard-off verified).
- **Two security problems found** that the design must handle (section 4):
  - IronRDP **accepts any server certificate by default**.
  - The authentication library **writes the password to debug logs**.
- **Not yet tested:** Windows hosts, Android builds, the Flutter UI, and Windows Credential Manager. This sandbox can't download Flutter, the Android SDK or extra Rust toolchains, and has no Windows machine. These move to Stage 2 checks (section 9).

---

## 2. Decisions Taken (Recommended Defaults — Change Any You Disagree With)

| # | Decision | Chosen default |
|---|----------|----------------|
| 1 | Engine | **E3 — IronRDP** (Rust) |
| 2 | UI framework | **U1 — Flutter** + flutter_rust_bridge |
| 3 | Suggestions | **All approved:** S1–S13, F1–F13, T1–T4, plus the security additions in section 6 |
| 4 | Network scope | Local network directly; **outside access only through a VPN** (WireGuard/Tailscale) with two-factor login. No public RDP port. |
| 5 | Linux hosts | **Supported** (xrdp / GNOME Remote Desktop), secondary to Windows hosts. Verified working with xrdp in the spike. |
| 6 | Distribution | **Windows:** signed MSI installer + portable ZIP. **Linux:** .deb + AppImage (portable); Flatpak later. **Android:** signed APK on GitHub first, F-Droid later (AAB if Play Store). Details in Stage 10 and SA-10. |
| 7 | Android minimum | **Android 10 (API 29)**. Android's security guidance recommends this because it blocks background apps from reading the clipboard. |
| 8 | Android background | Foreground-service notification with a **5-minute grace period**, then a clean disconnect with one-tap reconnect. |
| 9 | Unattended defaults | **On:** biometric/PIN before using a saved password, and idle disconnect. |
| — | Project licence | **Apache-2.0** (compatible with IronRDP MIT/Apache-2.0, Flutter BSD, flutter_rust_bridge MIT) |

---

## 3. Recommended Technology Stack

| Layer | Choice | Licence | Why |
|-------|--------|---------|-----|
| RDP engine | IronRDP (Devolutions). Prototype: git commit `f01f9b5` (28 Oct 2026). App: published crates.io releases (see `Cargo.lock`) | MIT OR Apache-2.0 | Memory-safe Rust; actively maintained (commit the day before this report); NLA/CredSSP, TLS, clipboard channel, RD Gateway crate (`ironrdp-mstsgu`) |
| TLS | rustls (via `ironrdp-tls`) | Apache-2.0/MIT/ISC | Spike negotiated **TLS 1.3**; supports a custom certificate check for pinning |
| Clipboard | `ironrdp-cliprdr` + `ironrdp-cliprdr-native` | MIT/Apache-2.0 | Native backends for **Windows** and **Linux (X11 and Wayland data-control)**. GNOME doesn't offer data-control, so there it falls back to X11/XWayland (to be tested in Stage 6). **No Android backend**, so Android needs a Kotlin bridge. |
| Credential vault | `keyring` crate (Windows Credential Manager, Linux Secret Service); Android Keystore via Kotlin | MIT/Apache-2.0 | Save, read and delete verified on Linux (section 5) |
| UI | Flutter + flutter_rust_bridge 2.13.0 (pinned) | BSD-3 / MIT | One UI for all three platforms; the bridge officially supports Android, Windows and Linux |
| Android-only parts | Kotlin | — | Keystore, clipboard bridge, foreground service, `FLAG_SECURE` |
| Packaging | WiX (MSI), AppImage, Flatpak, Gradle (APK/AAB) | various OSS | — |
| CI & security | GitHub Actions (Windows + Ubuntu), CodeQL, Dependabot, OSV-Scanner, gitleaks, Syft, cargo-audit, cargo-deny | OSS | — |

**Why not the alternatives:**
- **FreeRDP:** C code with many 2026 memory-safety CVEs.
- **Built-in Windows RDP control:** Windows-only and not open source.
- **RustDesk:** AGPL licence and a different protocol.
- **VNC:** weak security on its own.

---

## 4. Findings That Change the Design

| ID | Finding | Evidence | Required action |
|----|---------|----------|-----------------|
| **SF-1** 🔴 | IronRDP's default certificate policy is `DangerouslyAcceptInvalidCertificate` (accepts any server certificate). The bundled viewer also connected to the test host without any certificate prompt. | `ironrdp-tls/src/lib.rs`; spike test V1 | Never use the default. The app **must** use our own pinning check (proven in spike T1–T4) with a first-use approval prompt. Add a regression test that fails if any code path uses the dangerous policy. |
| **SF-2** 🔴 | The CredSSP library (`sspi` 0.23.1, the latest release) logs the credential buffer at **debug** level, so the **password appears hex-encoded in debug and trace logs**. At info level it does not. | Spike T7: password found at `debug` and `trace`; 0 occurrences at `info` or with `sspi=off` | Release builds compile out debug/trace logging and always disable `sspi` logs. Add a CI test that greps logs for a test password. **Report upstream to Devolutions privately** (responsible disclosure). |
| **SF-3** 🟠 | The blocking connect API has no timeout. Connecting to an unreachable host hung until killed. | Spike: stock example hung; our version with a timeout failed cleanly in **10 s** | Every network step (TCP, TLS, NLA, gateway) gets a configurable timeout. |
| **SF-4** 🟠 | When the requested screen size differs from the host's, the server re-sends a *Demand Active* message mid-session. The simple example crashed on it. | Spike: first run failed with "unsupported Share Control PDU (Server Demand Active PDU)" | Use IronRDP's full session engine (`ironrdp-client` crate), which handles reactivation. Add a resize/reactivation test. |
| **SF-5** 🟡 | No Android clipboard backend exists in IronRDP. | `ironrdp-cliprdr-native` only builds Windows/Linux backends (stub elsewhere) | Write a small Kotlin clipboard bridge. It reads only while the app has focus (Android 10+ rule) and marks host text as sensitive. |
| **SF-6** 🟡 | Windows Home can't host RDP; only Pro/Enterprise/Education can. | Confirmed by multiple current guides | Show this in the host-setup checklist (F12) and the docs. |

---

## 5. Proof-of-Concept Results (Linux client, 29 Oct 2026)

**Test setup:**
- **Client:** IronRDP built from source with Rust 1.97 (the project's pinned toolchain couldn't be downloaded here), plus a small test client I wrote (`spike_secure.rs`, attached).
- **Host A:** FreeRDP 3 shadow server with **NLA required**. This is an independent RDP implementation.
- **Host B:** xrdp 0.9.24 on Ubuntu 24.04 with the TLS security layer, used for clipboard tests.

| Test | Result | Detail |
|------|--------|--------|
| NLA (CredSSP) login | ✅ Pass | Server selected HYBRID (NLA); "Connected with success" |
| Encryption | ✅ Pass | TLS 1.3 negotiated |
| Remote screen | ✅ Pass | 1280×800 desktop captured; about 900 frames received in 8 s |
| T1 — unknown certificate, user hasn't approved | ✅ Pass | Connection refused before login; fingerprint shown for approval |
| T2 — approve on first use | ✅ Pass | SHA-256 fingerprint pinned; connected |
| T3 — reconnect, same certificate | ✅ Pass | Pin matched; connected |
| T4 — host certificate changed | ✅ Pass | **Blocked**: "MISMATCH expected … got …"; no credentials sent |
| T5 — wrong password | ✅ Pass | Clear error: `STATUS_LOGON_FAILURE` |
| T6 — host offline (port closed) | ✅ Pass | Clear error: "Connection refused" |
| T6b — host unreachable | ✅ Pass (with fix) | Timed out cleanly after 10 s (SF-3) |
| T7 — password in logs | ⚠️ Partial | Not in plain text at any level. Present as hex at debug/trace (SF-2). Clean at info and with `sspi=off`. |
| C1 — clipboard, local → host | ✅ Pass | Text copied locally appeared in the host clipboard |
| C2 — clipboard, host → local | ✅ Pass | Text copied on the host appeared locally |
| C3 — clipboard turned off | ✅ Pass | Host clipboard unchanged; session kept running |
| V1 — bundled viewer certificate check | ❌ Unsafe default | Connected with no certificate prompt (SF-1) |
| K1 — save / read / delete password in Linux keyring | ✅ Pass | Saved, read back, deleted ("no entry" after delete); 0 plaintext copies found in the home folder |

**Not tested (environment limits), carried into Stage 2 checks:**
- A real **Windows** host with NLA (Kerberos/NTLM against Windows).
- An **Android** build.
- The **Flutter** UI and flutter_rust_bridge.
- **Windows Credential Manager** and the **Android Keystore**.
- **Wayland** clipboard.
- **RD Gateway**.
- Behaviour when no keyring is available (I8).

---

## 6. Security Design (Final)

**Baseline (from approved suggestions):**
- **S1:** NLA + TLS 1.2+ only; legacy RDP security refused.
- **S2:** pinned SHA-256 certificate fingerprints with a first-use prompt. Handshake signatures are still verified, so a copied certificate without its private key fails (implemented in the spike).
- **S3:** all redirection off except an opt-in clipboard.
- **S4–S5:** OS vault storage; biometric/PIN before a saved password is used.
- **S6:** clipboard size cap, indicator, and one-direction option.
- **S7:** idle disconnect.
- **S8:** signed releases, checksums, SBOM, `SECURITY.md`.
- **S9–S12:** Linux file permissions; Android `FLAG_SECURE`, no cloud backup of credentials, signing key kept offline.
- **S13:** a dedicated non-admin host account for unattended access.

**Added from the security review:**
- **SA-1:** VPN with two-factor login required for any access from outside the local network. No public port 3389.
- **SA-2:** written **threat model** (STRIDE) before Stage 4.
- **SA-3:** **fuzz testing** of everything the server sends us (IronRDP already ships `ironrdp-fuzzing`; we add our own parsers).
- **SA-4:** **host-hardening checklist**: account lockout policy, Windows Update, NLA enforced on the host, firewall limited to the VPN subnet.
- **SA-5:** **independent security review** before the first public release.
- **SA-6 (from SF-1):** test that no code path uses `DangerouslyAcceptInvalidCertificate`.
- **SA-7 (from SF-2):** logging hard cap at info in release builds, `sspi` logs always off, and a CI "password-in-logs" test.
- **SA-8:** reconnect never retries after an authentication failure (avoids account lockout, F13).

**SA-9: No secrets or personal data in the public source code (required before any GitHub release):**
- **No passwords, usernames, IP addresses or hostnames in code.** Test credentials are generated at test time or read from environment variables / GitHub Actions Secrets, never written in files.
- **Examples use reserved placeholder addresses only:** `192.0.2.x` and `example.com`.
- **Your personal files are never committed.** `.gitignore` excludes profiles, certificate pins, logs, `.env` files, keystores and signing keys.
- **Automatic secret scanning:** gitleaks runs as a pre-commit hook and in CI, so a commit containing a secret is blocked.
- **Commit identity:** commits use your GitHub *noreply* address, so your personal email isn't published in the git history.
- **Build paths stripped:** release builds remove local folder paths from binaries (`--remap-path-prefix`), so your computer's user/folder names aren't embedded.
- **Signing keys** (Android keystore, Windows code-signing) live only in GitHub Actions Secrets or offline, never in the repository.
- **Private until reviewed:** the repository stays private until a full history scan (gitleaks over every commit) is clean. Only then is it made public.
- **Passwords never on the command line.** The real app takes them only through the masked field or the vault (the spike's `-p` flag was for testing only), so they don't end up in shell history or process lists.

**SA-10: Portable mode.** A portable copy (for example on a USB drive) can be lost or copied, so:
- Its `data` folder holds **no secrets**: only profiles and certificate pins.
- Passwords are never written to the drive.
- Save Password is off by default. If you turn it on, the password goes into the current PC's vault only, the app warns you, and it doesn't travel with the drive.
- A changed or tampered pin file can't silently trust a new server: a mismatch still blocks the connection.

**Known residual risks (documented, not fully fixable by the app):**
- A compromised host can read anything sent over the clipboard.
- The first connection relies on the user checking the fingerprint.
- Android malware with accessibility permission, or a rooted phone, can read the screen.
- Bugs in the engine (IronRDP is younger and less examined than FreeRDP).
- The app can't make host-side settings secure.

---

## 7. Architecture

```
┌──────────────────────── Flutter UI (Dart) ────────────────────────┐
│ Profiles · Connect form · Session view (texture) · Settings ·     │
│ Touch/keys bar (Android) · Status & errors · Clipboard indicator  │
└───────────────▲───────────────────────────────────────▲───────────┘
                │ flutter_rust_bridge (generated)       │ platform channel
┌───────────────┴──────── Rust core (shared) ───────┐   ┌┴──────── Kotlin (Android) ────────┐
│ session: IronRDP connector + ironrdp-client       │   │ Keystore vault · clipboard bridge │
│ security: cert pinning store, timeouts, policy    │   │ foreground service · FLAG_SECURE  │
│ clipboard: cliprdr + native backends (Win/Linux)  │   └───────────────────────────────────┘
│ profiles: JSON store (no secrets), import/export  │
│ vault: keyring crate (Win Credential Mgr / Linux) │
│ logging: redacted, capped at info in release      │
└───────────────────────────────────────────────────┘
```

- **Saved locally (no secrets):** profiles and certificate pins in the user config folder, readable only by the user.
- **Saved only in the OS vault:** passwords.

---

## 8. Development Roadmap (Stages 2–10)

Each stage follows your workflow: plan → build → test → fix → record → commit, and nothing moves on with critical errors open.

| Stage | Objective | Key acceptance criteria |
|-------|-----------|-------------------------|
| **2 Foundation** | Repo, Cargo workspace, Flutter app, bridge, CI on Windows + Ubuntu + Android build | "Hello from Rust" runs on all 3 platforms; CI green; CodeQL/OSV/gitleaks/cargo-deny wired in; licence and `SECURITY.md` present; SA-9 in place (`.gitignore`, gitleaks pre-commit hook, noreply commit email, repo private). **Includes the Windows-host, Android-build and Flutter checks the spike couldn't run.** |
| **3 UI** | Main window, connect form, profiles list, status, errors, settings, themes; Android touch layout | Masked password, Save Password checkbox, layouts usable on phone + desktop; UI tests pass |
| **4 Connection engine** | IronRDP session, TOFU pinning, timeouts, reactivation, graceful disconnect; remote screen display; **keyboard, mouse and touch input** (direct-touch mode and special-keys bar on Android; trackpad mode and pinch-zoom follow in Stage 7) | Spike tests T1–T7 automated; NLA against a **Windows** host passes; no dangerous certificate policy (SA-6) |
| **5 Credentials** | Vaults on all 3 OSes, biometric gate, delete | Save/read/delete on each OS; no plaintext copies; deleted password can't be retrieved; no-keyring path refuses to save |
| **6 Clipboard** | Two-way text, toggle, one-direction, size cap, Android bridge | C1–C3 on Windows, Linux X11, Linux Wayland, Android; stops at session end |
| **7 Profiles** | Save/edit/delete/reconnect, history, import/export, unattended checklist, Wake-on-LAN; Android trackpad mode and pinch-zoom (F8) | Corrupt-file and interrupted-write tests pass; exports contain no secrets |
| **8 Reliability** | Every error path, auto-reconnect, Android background handling | Error-matrix tests; no secrets in any message or log (SA-7) |
| **9 Security verification** | Threat-model review, fuzzing, static analysis, licence audit | No open critical/high issues; fuzzers run without crashes for the agreed time |
| **10 Release** | Proper installation packages: **Windows** signed MSI installer (Start-menu and optional desktop shortcut, Add/Remove Programs entry, in-place upgrade, clean uninstall); **Linux** .deb package plus AppImage (app-menu entry and icon; Flatpak later); **Android** signed APK (plus AAB if Play Store is used); **portable versions**: Windows portable ZIP (signed EXE, runs without installing or admin rights) and Linux AppImage. In portable mode, profiles and certificate pins are kept in a `data` folder next to the app, never passwords; Save Password stays off by default and, if turned on, stores the password only in that PC's vault (with a warning); docs; make repo public | Clean install / upgrade / uninstall on each OS; credentials removed on uninstall (where the OS allows); docs complete; **full-history secret scan clean and binaries contain no local paths or personal data before going public** |

---

## 9. Test Strategy

- **Unit tests (Rust):** pin store, profile store, timeout logic, log redaction, reconnect rules.
- **Unit and widget tests (Dart):** UI state and error display.
- **Integration tests on Linux CI:** the spike's NLA host (FreeRDP shadow) and clipboard host (xrdp) run as automated tests: T1–T7 and C1–C3.
- **Windows host tests:** Windows Pro VM (Hyper-V), covering NLA, certificate prompt, clipboard, Credential Manager.
- **Android tests:** emulator (API 29 and latest) plus two real devices, covering touch input, Keystore, clipboard focus rules, background grace period, Wi-Fi ↔ mobile data switch.
- **Security tests:** password-in-logs grep, dangerous-policy check, fuzzing, `cargo audit` / `deny` / OSV / CodeQL on every pull request.
- **Manual tests:** real hosts on your network, including unattended access and Wake-on-LAN.
- **Rule:** any test that can't be run is reported as **not run**, never as passed.

---

## 10. Known Limitations

- Only **text** clipboard in v1; images and files are evaluated later.
- Android is a viewer only; it can't be a host.
- Windows Home PCs can't be hosts.
- Wake-on-LAN works only on the local network or through the VPN.
- IronRDP is younger than FreeRDP, so it has fewer real-world deployments.
- Flutter adds app size (estimated, to be measured in Stage 2).
- The bundled IronRDP viewer is a reference only; we build our own UI.

---

## 11. Approval

Stage 1 was **approved on 29 October 2026** with the defaults in section 2,
plus the additions requested afterwards: installation packages, portable versions,
source-code privacy rules (SA-9) and portable-mode rules (SA-10).

Still open:
1. **GitHub repository:** to be created by the owner as a **private** repository
   (suggested name `cipher-desk`); it stays private until the SA-9 checks pass.
2. **Private report to Devolutions about SF-2:** to be drafted for the owner's approval.

---

## Sources

- [IronRDP repository (GitHub)](https://github.com/Devolutions/IronRDP)
- [IronRDP documentation](https://www.mintlify.com/Devolutions/IronRDP/introduction)
- [flutter_rust_bridge (docs.rs)](https://docs.rs/crate/flutter_rust_bridge/latest)
- [Android — Secure clipboard handling](https://developer.android.com/privacy-and-security/risks/secure-clipboard-handling)
- [MITRE ATT&CK T1414 — Clipboard Data](https://attack.mitre.org/techniques/T1414)
- [Windows 11 Home cannot host Remote Desktop (iTechGuides)](https://www.itechguides.com/?p=353808)
- [FreeRDP 3.23.0 release notes — CVE list](https://sourceforge.net/p/freerdp/mailman/message/59300559/)
- [CVE-2026-44420 — FreeRDP (Action1)](https://www.action1.com/vulnerabilities/cve-2026-44420/)
- Spike source: `spike_secure.rs` (attached); tests run in this workspace on 29 Oct 2026
