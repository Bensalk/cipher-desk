# Changelog

Download any version from the [Releases page](https://github.com/Bensalk/cipher-desk/releases).

## v0.1.0 (30 Oct 2026, test release)

Not yet independently security-reviewed; see [docs/RELEASING.md](docs/RELEASING.md).


- First version: a secure Remote Desktop (RDP) viewer for Windows, Linux and Android, with one shared Rust core built on IronRDP
- Secure connections only: Network Level Authentication (NLA) and TLS 1.2 or 1.3; older, unprotected connections are refused
- Certificate check on the first connection, then the certificate is pinned; a changed certificate blocks the connection before any password is sent
- Optional saved passwords, kept only in Windows Credential Manager, the Linux keyring or the Android Keystore, with Windows Hello or the Android screen lock before use
- Optional two-way text clipboard, off by default, with a one-direction choice per connection
- Keyboard and mouse, touch gestures, trackpad mode, pinch to zoom up to 4×, and a key bar with Ctrl+Alt+Del, Windows key, arrows and F1–F12
- Saved connections with search, Wake-on-LAN, a readiness check, connection history, and export / import without passwords
- Reconnects by itself after a network drop (up to 3 tries, same certificate); never retries a wrong password
- Disconnects when idle (30 minutes by default) and, on Android, 5 minutes after you leave the app
- Clear error messages that say what to do next, with a help page listing all of them
- Light and dark themes; screenshots blocked on Android by default
- Packages: Windows MSI and portable ZIP, Linux .deb and AppImage, Android APK
- Ctrl, Alt and Shift on the key bar stay held until tapped again, for key combinations on touch screens
- Phones and tablets get a landscape remote desktop (up to 1920×1080)
- Asks before disconnecting when you press Back during a session
- A password is only asked for in the connection form when you choose to save it
