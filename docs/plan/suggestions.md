# Secure Open-Source Remote Desktop Viewer — Suggestions for Review

**Status:** Awaiting approval (Stage 1, pre-plan)
**Date:** 28 October 2026
**Target platforms:** Windows, Linux and Android

Approve, reject or modify items by ID (e.g. "approve all except F6, S5").

---

## 0. Platform Considerations

- **One shared codebase** for Windows, Linux and Android: a shared RDP core plus one cross-platform UI.
- **All three platforms are viewers (clients).** Android can't act as an RDP host. Windows and (optionally) Linux machines are the computers you connect *to*.
- **Credential storage** uses each OS's protected vault:
  - Windows: Credential Manager
  - Linux: Secret Service (GNOME Keyring / KWallet)
  - Android: Android Keystore (hardware-backed key that encrypts the saved password)
- **Clipboard rules differ:**
  - Linux: X11 and Wayland behave differently, and Wayland restricts clipboard access by background apps.
  - Android 10+: apps can read the clipboard only while in the foreground. Android 13+ shows a "pasted from clipboard" notice.
- **Input differs on Android:** touch has to map to mouse actions, and the user needs an on-screen keyboard with special keys (Ctrl, Alt, Win, Esc, F-keys). External keyboards and mice should also work.
- **Packaging:** MSI (Windows); AppImage / Flatpak / .deb (Linux); APK / AAB (Android).
- **Linux machines as hosts** (optional) need an RDP server: xrdp, or GNOME Remote Desktop.
- **Unattended access** is built into RDP: nobody has to be at the host to accept the connection. The host only needs to be on, have Remote Desktop enabled, and have an account with a password. RDP signs into that account and locks the host's local screen; it does not show someone's live screen the way AnyDesk or TeamViewer do. Saved passwords (S4) work with unattended profiles, protected by S5, S7 and S13.

---

## 1. Engine Choice (Key Decision)

| ID | Option | Pros | Cons |
|----|--------|------|------|
| **E3** ⭐ Recommended | **IronRDP** (Rust, MIT/Apache-2.0, Devolutions) | One memory-safe core that compiles for Windows, Linux and Android; security-focused and fuzzed; includes a clipboard (`cliprdr`) channel | Less mature as a complete end-user client, so it needs a proof-of-concept spike in Stage 1 (see I9) |
| **E2** | **FreeRDP 3.x** (C, Apache-2.0) | Most mature open RDP stack; runs on Windows, Linux and Android; very active (3.26.0, May 2026) | Many 2026 security fixes (one release fixed 11 CVEs; others fixed heap overflows and use-after-free bugs with public PoCs), so updates must ship fast; building C for Android and binding it to the UI adds complexity |

**Rejected:**
- Built-in Windows RDP control (MSTSCAX): Windows-only, and not open source.
- RustDesk: AGPL licence and a different protocol.
- Plain VNC: weak security on its own.
- Any custom protocol.

---

## 2. UI Framework Choice (Key Decision)

| ID | Option | Pros | Cons |
|----|--------|------|------|
| **U1** ⭐ Recommended | **Flutter** (BSD) + **flutter_rust_bridge** (MIT) to the Rust core | Mature on Windows, Linux and Android; strong touch and mobile UI; fast texture rendering for the remote screen; the same Flutter + Rust pattern is proven in existing remote-desktop apps | Two languages (Dart + Rust); larger app size |
| **U2** | **Slint** (Rust-native UI) | All-Rust, lightweight, native rendering on all three platforms | Smaller ecosystem and younger Android support; check the licence (GPLv3 or Slint's royalty-free licence) |
| **U3** | **Tauri 2** (Rust + web UI) | Web-based UI, small desktop builds, supports Android | The remote screen has to be drawn through a web canvas, which is slower for continuous video-like updates |

---

## 3. Potential Issues

| ID | Issue |
|----|-------|
| **I1** | Windows **Home** can't host RDP sessions. Windows host PCs need Pro, Enterprise or Server. |
| **I2** | Never expose port 3389 to the internet. For remote access, use a VPN (WireGuard or Tailscale, both of which have Windows, Linux and Android apps) or an RD Gateway. |
| **I3** | Most hosts use self-signed certificates, so the app needs a trust-on-first-use prompt plus pinning (see S2). |
| **I4** | Turning clipboard sharing on or off mid-session may require a reconnect, depending on the engine. |
| **I5** | Unsigned Windows installers trigger SmartScreen warnings. Apply for free open-source code signing (e.g. SignPath Foundation). |
| **I6** | High-DPI scaling, multi-monitor setups, and phone/tablet screen sizes all need testing. |
| **I7** | **Linux — Wayland vs X11:** clipboard monitoring, keyboard grabbing (e.g. Alt+Tab going to the remote PC) and full-screen behave differently. Test on GNOME (Wayland) and an X11 desktop. |
| **I8** | **Linux — no keyring available** (headless or minimal desktops): the app must refuse to save passwords, never fall back to a plaintext file, and tell the user why. |
| **I9** | **Engine maturity (if E3):** before full build, run a Stage 1 spike to confirm NLA, certificate validation and two-way clipboard on all three platforms. If it fails, switch to E2. |
| **I10** | **Linux distribution differences** (library versions, desktop environments). AppImage/Flatpak reduce this but need sandbox permissions for network and keyring. |
| **I11** | **Android — app in background:** Android may cut the network when the app is in the background. Either keep the session alive with a visible foreground-service notification, or disconnect cleanly and offer a quick reconnect. |
| **I12** | **Android — clipboard:** Host-to-phone copying works while the session is on screen. Phone-to-host only syncs while the app is in the foreground, so the app should send the latest clipboard text when it returns to the foreground (if enabled). |
| **I13** | **Android — mobile networks:** switching between Wi-Fi and mobile data drops the connection, and mobile data adds latency and data usage. The app needs auto-reconnect and quality settings (colour depth, codec). |
| **I14** | **Android — device fragmentation:** different Android versions, screen sizes and vendor battery-saver settings. Set a minimum version (e.g. Android 10+) and test on several devices. |
| **I15** | **Unattended host setup:** the host must stay reachable when no one is present. Requirements: Windows Pro, Enterprise or Server (or Linux with xrdp / GNOME Remote Desktop); sleep and hibernate off, or Wake-on-LAN configured; Windows updates scheduled so they don't leave the host stuck at a restart; an account with a password (RDP blocks blank-password accounts by default). Wake-on-LAN normally works only on the same local network, or through a VPN or an always-on device on that network. |

---

## 4. Security Measures

| ID | Measure |
|----|---------|
| **S1** | Require NLA and TLS 1.2 or later. Refuse to fall back to legacy RDP security. |
| **S2** | Pin each server's certificate fingerprint and show a prominent warning if it changes (protects against man-in-the-middle attacks). |
| **S3** | Turn off drive, printer, USB and audio redirection by default. Only the clipboard is on, and only when the user allows it. |
| **S4** | Store credentials in the OS vault under an app-specific name (Credential Manager / Secret Service / Android Keystore). Wipe passwords from memory after use, and redact them from logs. |
| **S5** | Optionally require OS authentication before a saved password can be used: Windows Hello / PIN, Linux keyring unlock, Android biometric or screen-lock prompt. |
| **S6** | Clipboard size cap, a visible "clipboard active" indicator, and a one-direction-only option. On Android, mark copied host text as sensitive so it's hidden from clipboard previews. |
| **S7** | Disconnect after a configurable idle time. |
| **S8** | Signed releases, published checksums, an SBOM, and a `SECURITY.md` file explaining how to report vulnerabilities. |
| **S9** | On Linux, config and profile files are readable only by the owner (permission `0600`) and stored in standard user folders (`~/.config`). Flatpak permissions limited to network + Secret Service. |
| **S10** | On Android, block screenshots and recent-apps thumbnails of the remote screen (`FLAG_SECURE`, on by default and switchable in settings). |
| **S11** | On Android, exclude credentials and profiles from cloud backup and device transfer. Request only the permissions the app needs (network; plus notification if I11 uses a foreground service). |
| **S12** | Keep the Android release signing key offline and separate from connection authentication. Owning the signing key or the APK never grants remote access. |
| **S13** | **Unattended profiles:** use a dedicated host account with only the permissions it needs (not an administrator). When a profile has a saved password, turn on S5 (biometric/PIN before use) and S7 (idle disconnect) by default; the user can turn them off with a warning. |

---

## 5. Extra Features

| ID | Feature |
|----|---------|
| **F1** | Quick-connect bar and profile search |
| **F2** | Clipboard setting per profile |
| **F3** | RD Gateway support |
| **F4** | Profile export/import (passwords never exported), which also lets you move profiles between Windows, Linux and Android |
| **F5** | Local connection log containing no secrets |
| **F6** | Update checker that verifies signatures (on Linux and Android, defer to Flatpak, F-Droid, Play Store or the package manager where used) |
| **F7** | Dark and light themes (follow the OS theme on all platforms) |
| **F8** | Android touch controls: direct-touch and trackpad modes, pinch-to-zoom, two-finger scroll, long-press for right-click |
| **F9** | Android special-keys bar (Ctrl, Alt, Win, Esc, Tab, F1–F12, Ctrl+Alt+Del) plus external keyboard and mouse support |
| **F10** | Remote resolution that fits the phone or tablet screen, with a scaling option |
| **F11** | Wake-on-LAN: wake a sleeping host (MAC address saved in the profile) before connecting |
| **F12** | "Unattended-ready" checklist for each host profile: host reachable, RDP enabled, NLA on, certificate pinned, password saved, Wake-on-LAN tested |
| **F13** | Auto-reconnect after network drops using the saved password, with a retry limit (no reconnect after an authentication failure, to avoid account lockout) |

---

## 6. Open-Source Tools

| ID | Area | Tools |
|----|------|-------|
| **T1** | App & tests | **Core:** Rust + IronRDP (if E3) or FreeRDP (if E2), `cargo test`. **UI:** Flutter + flutter_rust_bridge (U1), Slint (U2) or Tauri 2 (U3). **Vaults:** `keyring` crate (Windows/Linux), Android Keystore via platform channel |
| **T2** | Packaging | Windows: WiX Toolset (MSI). Linux: AppImage + Flatpak, optionally .deb. Android: APK + AAB built with Gradle, Android NDK and `cargo-ndk` |
| **T3** | CI & security | GitHub Actions with Windows + Ubuntu runners (Ubuntu also builds Android), CodeQL, Dependabot, OSV-Scanner, gitleaks, Syft (SBOM); cargo-audit and cargo-deny if E3 |
| **T4** | Test hosts & devices | Windows: Hyper-V VMs / Windows Sandbox. Linux: VMs running Ubuntu (GNOME/Wayland) and an X11 desktop (e.g. Xfce); xrdp / GNOME Remote Desktop for Linux-as-host tests. Android: emulator plus at least two real devices (phone + tablet, different Android versions) |

---

## 7. Decisions Needed Before the Final Plan

1. **Engine:** E3 (recommended) or E2?
2. **UI framework:** U1 Flutter (recommended), U2 Slint or U3 Tauri 2?
3. **Approvals:** Which S, F and T items to include?
4. **Network scope:** Local network only, or internet access too? (This decides whether I2 needs a VPN or RD Gateway.)
5. **Linux scope:** Should connecting *to* Linux hosts (xrdp / GNOME Remote Desktop) also be supported and tested?
6. **Distribution:**
   - Linux: AppImage, Flatpak and/or .deb?
   - Android: GitHub APK, F-Droid and/or Google Play?
7. **Android minimum version:** Android 10+ (suggested) or another?
8. **Android background behaviour (I11):** keep sessions alive with a notification, or disconnect when the app is in the background?
9. **Unattended access defaults (S13):** should biometric/PIN unlock and idle disconnect be on by default for profiles with saved passwords?

---

## Sources

- [FreeRDP 3.26.0 release (Ubunlog)](https://en.ubunlog.com/?p=58101)
- [FreeRDP 3.23.0 release notes — CVE list (FreeRDP mailing list)](https://sourceforge.net/p/freerdp/mailman/message/59300559/)
- [CVE-2026-44420 — FreeRDP (Action1)](https://www.action1.com/vulnerabilities/cve-2026-44420/)
- [IronRDP repository (GitHub)](https://github.com/Devolutions/IronRDP)
- [IronRDP documentation](https://www.mintlify.com/Devolutions/IronRDP/introduction)
