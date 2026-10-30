# Changelog

Download any version from the [Releases page](https://github.com/Bensalk/cipher-desk/releases).

## v0.2.0 (30 Oct 2026, test release)

Not yet independently security-reviewed; see [docs/RELEASING.md](docs/RELEASING.md).

- Three connection methods, chosen per connection, with a two-question helper: **Private Remote Desktop**, **Share screen** and **VNC** ([which one to use](docs/CONNECTION-METHODS.md))
- **Share screen** (Windows): a new *Share screen* page lets your phone or another computer see and control this computer's screen while it stays visible here, on any Windows edition. Encrypted, with an identity code to check, a generated password kept in Windows' secure storage, and a 15-minute pause after 5 wrong passwords
- Share screen options: start sharing when Cipher Desk opens, open Cipher Desk when you sign in to Windows, and a "Your screen is being shared" banner with Stop
- **VNC**: connect to computers running a VNC program such as TightVNC, only on your own network or Tailscale, because VNC isn't encrypted
- Windows: the Cipher Desk window is now hidden from screenshots and screen sharing (setting *Block screenshots*, on by default), and always while sharing
- Step-by-step tutorials for each method: [Private Remote Desktop](docs/methods/PRIVATE-REMOTE-DESKTOP.md), [Share screen](docs/methods/SHARE-SCREEN.md), [VNC](docs/methods/VNC.md)

## v0.1.0 (30 Oct 2026, test release)

Not yet independently security-reviewed; see [docs/RELEASING.md](docs/RELEASING.md).

- First version: a secure Remote Desktop (RDP) viewer for Windows, Linux and Android, with one shared Rust core built on IronRDP
- Secure connections only: Network Level Authentication (NLA) and TLS 1.2 or 1.3; older, unprotected connections are refused
- Certificate check on the first connection, then the certificate is pinned; a changed certificate blocks the connection before any password is sent
- Optional saved passwords, kept only in Windows Credential Manager, the Linux keyring or the Android Keystore, with Windows Hello or the Android screen lock before use
- Optional two-way text clipboard, off by default, with a one-direction choice per connection
- Keyboard and mouse, touch gestures, trackpad mode, pinch to zoom up to 4×, two-finger scrolling, and a key bar with Ctrl+Alt+Del, Windows key, arrows and F1–F12
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
