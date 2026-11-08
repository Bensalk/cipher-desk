# Changelog

Download any version from the [Releases page](https://github.com/Bensalk/cipher-desk/releases).

## v0.3.3 (8 Nov 2026, test release)

Builds on v0.3.2. The About screen shows **0.3.3**.

- **The app now stays with it for longer when the network drops**, instead
  of giving up after about 17 seconds. A relayed connection (for example
  over Tailscale when it can't connect the two computers directly) can take
  longer than that to recover, even though the computer is reachable a
  moment later by other means. Now retries for about 1.5 minutes before
  showing "Connection lost."
- **Fixed a file copy-paste error:** pasting a folder or several files
  copied from the host could show Windows' own "Error Copying File or
  Folder: The object invoked has disconnected from its clients." Each file
  was being put on the clipboard separately as it arrived, which could land
  mid-paste. Now waits for the whole batch to finish arriving before
  putting it on the clipboard once.

## v0.3.2 (8 Nov 2026, test release)

Builds on v0.3.1. The About screen shows **0.3.2**.

- **Fixed: turning on "Copy and paste for new connections" in Settings did
  nothing for a connection you had saved earlier.** That setting only
  applies to connections created after it — each existing connection keeps
  its own copy-and-paste setting. If it's off when you connect, there's
  now a **"Turn on"** button right on the notice, which turns it on for
  that connection and reconnects immediately — no need to find the
  connection's edit screen.
- **Online / offline dot on each saved connection.** A small green dot on
  its icon means the computer answers right now; red means it doesn't
  (off, asleep, not sharing, or not reachable from here). Checked every
  15 seconds; nothing is sent to the computer besides the same quick
  connection check "Check readiness" already did.

## v0.3.1 (8 Nov 2026, test release)

Builds on v0.3.0. The About screen shows **0.3.1**.

- **Keyboard shortcuts and full-screen controls no longer sit fixed on top of
  the screen.** Each is now a small round button you can drag anywhere;
  tap it to open the keys (or the full-screen/view buttons), tap again to
  close them. The button itself always stays reachable, so full screen and
  disconnect are never blocked off.

## v0.3.0 (8 Nov 2026, test release)

Builds on v0.2.9. The About screen shows **0.3.0**.

- **Any Windows display scale on the shared computer.** The always-on sharing
  service now knows about display scaling (125 %, 150 %, ...). Before, the
  shared picture could be cut off (the taskbar missing) and clicks could land
  in the wrong place on such a screen.
- **Copy and paste is easier to find and turn on.** The connection setting is
  now called *Copy and paste (text and files)* (it was "Share clipboard (text
  only)", although it also controls files). A new *Share screen* connection
  starts with it on, and a connection that has it off says how to turn it on.
- **More reliable file copy.** Right after Ctrl+C, File Explorer can hold the
  clipboard for a moment; Cipher Desk now waits for it instead of missing the
  copy, on both computers.

## v0.2.9 (7 Nov 2026, test release)

Builds on v0.2.8. The About screen shows **0.2.9**.

- File copy and paste now handles **folders** (with everything inside them)
  and **files of any size**. Files travel in small pieces written straight to
  disk, like AnyDesk, so large downloads and .zip files work too. The viewer
  shows "Receiving … from the remote computer" with the number and size of the
  files; paste with Ctrl+V once they have arrived. An unfinished file is never
  left behind if the copy stops half-way.
- Two-step verification setup shows a **QR code**: scan it with Google
  Authenticator ("Scan a QR code") instead of typing the key.

## v0.2.8 (7 Nov 2026, test release)

Builds on v0.2.7. The About screen shows **0.2.8**.

- **Copy and paste files, like AnyDesk** (Share screen, Windows): copy files in
  File Explorer with **Ctrl+C** on either computer, then press **Ctrl+V** in a
  folder on the other one. Works both ways while a session is open, also with
  the Cipher Desk window minimised. The connection's *Clipboard* setting must
  allow the direction (*Both ways* for both). Files only (not folders), up to
  100 MB each; a copy of each file is also kept in the *Received* folder.
- **Two-step verification with Google Authenticator** for always-on sharing:
  on the computer being shared, open *Share screen* and turn on **Two-step
  verification**, add the key to Google Authenticator, and confirm with a code.
  After that, a device connecting with the password is asked once for the
  6-digit code, then remembered for 30 days. Five wrong codes block code entry
  for 15 minutes. *Ask to connect* is not affected (someone at the computer
  accepts instead).

## v0.2.7 (7 Nov 2026, test release)

Builds on v0.2.6. The About screen shows **0.2.7**.

- **Ask to connect (no password)**, like AnyDesk: for a *Share screen*
  connection, choose **Ask to connect** in its ⋮ menu. On the other computer,
  where always-on sharing is turned on, a popup asks whether to allow it. The
  screen appears only after someone there clicks **Yes**; **No**, or no answer
  within 30 seconds, refuses it and the viewer shows "The other computer didn't
  allow it". Connecting with the password still goes straight in.
- Uses the port after the sharing port (3396 with the default 3395).

## v0.2.6 (7 Nov 2026, test release)

Builds on v0.2.5. The About screen shows **0.2.6**.

- Share screen on Windows: the Cipher Desk window on the shared computer now
  really shows up for the viewer. In v0.2.4 and v0.2.5 it could still stay
  hidden from the picture (while taking clicks) when the app had just started.

## v0.2.5 (7 Nov 2026, test release)

Builds on v0.2.4. The About screen shows **0.2.5**.

- **Smoother picture**: when a large part of the remote screen changes at once
  (a window opening, a page loading), the change now appears in one go instead
  of being drawn in bands from top to bottom. Most noticeable with *Best
  quality*. Video and scrolling keep moving as before.

## v0.2.4 (7 Nov 2026, test release)

Builds on v0.2.3. The About screen shows **0.2.4**.

- **Picture quality** setting per connection, like AnyDesk: *Best quality*,
  *Balanced* (the default) or *Speed*. Change it in the connection's settings or
  from the quality button in the session toolbar (changing it reconnects).
  - *Best quality* with Share screen is lossless: every pixel arrives exactly
    as on the other computer, so small text is perfectly sharp. It uses the most
    data, so it suits a fast connection.
  - *Balanced* is much sharper than before.
  - *Speed* sends the least data for slow connections; the picture is softer.
- **Less lag** with Share screen: the host always sends the newest picture
  instead of a queue of older ones, so a slow connection lowers the frame rate
  rather than adding delay. Up to about 30 pictures a second.
- **View modes** in the session toolbar: *Original size* (sharpest, one remote
  pixel per screen pixel; move the mouse to an edge to see more), *Shrink to
  fit* and *Stretch to fill*.
- **Full screen** now really fills the screen: on Windows the taskbar is hidden,
  and on Android the status and navigation bars are hidden.
- Share screen: the Cipher Desk window on the shared computer is now visible
  to the viewer and can be used. Before, it was hidden from the picture but still
  took clicks, so it could block the viewer without them seeing it. The sharing
  password is masked while someone is viewing. (*Block screenshots* on Windows
  now protects the remote screen you are viewing.)

## v0.2.3 (7 Nov 2026, test release)

Builds on v0.2.2. The About screen shows **0.2.3**.

- Share screen: keyboard and mouse now work when you connect through the
  always-on background mode. Windows was refusing every key and click
  ("access denied") even though the screen was showing.
- Share screen: *Start sharing* no longer fails with "port in use" while
  always-on sharing is running. It now says that always-on sharing is already
  on, so viewers can connect straight away.
- The diagnostic log now says which screen (desktop or sign-in screen) keys and
  clicks go to, and reports refused input once instead of once per key.

## v0.2.2 (5 Nov 2026, test release)

Builds on v0.2.1. The About screen shows **0.2.2**.

**Always-allow connections** — a connection can now be set to *Always allow* so the
Share screen host accepts it without asking for approval each time. Toggle it on the
connection's detail page. Use this for a phone or computer you own; keep the default
*Ask each time* for other devices.

**Custom Share screen password** — you can now replace the generated password on the
Share screen page with one you choose. Tap *Change password*; the new password is
saved in Windows Credential Manager like the generated one. Tap *Reset to generated*
to go back.

**Approval prompt on the host** — when a new device connects to Share screen in *Ask
each time* mode, a prompt now appears on the host computer asking whether to allow
it. If nobody answers within 30 seconds the connection is refused. The viewer sees a
clear "waiting for approval" message while this is happening.

**Clipboard** (Private Remote Desktop) — clipboard sharing is now fully two-way:
text you copy on the remote computer appears in this device's clipboard too. The
direction icons in the title bar show which way copying is active.

**File transfer** (Private Remote Desktop, Windows host) — drag a file onto the
session window, or use the new *Send file* button in the toolbar, to copy it to the
remote computer's desktop. Progress is shown in the toolbar. Files up to 256 MB; one
at a time. This is a first version — please test it and report any issues.

## v0.2.1 (31 Oct 2026, test release)

Supersedes the v0.2.0 test drafts. If you tested v0.2.0 and found Share screen
laggy or unresponsive to mouse/keyboard, that was the older draft — this build
fixes it. The About screen shows **0.2.1**.

- Share screen: the host now services mouse and keyboard **before** encoding each
  screen frame, so control works (it was being starved by the screen video)
- Much smoother picture: the viewer requests **RemoteFX**, so both Share screen and
  Private Remote Desktop send compact whole-frame updates instead of large bitmaps
  (no more pixel-by-pixel painting), at ~20 frames a second
- Viewer: **stretch to fill** the window (or keep aspect) and an **immersive
  fullscreen** mode with a floating exit button
- A privacy-safe Share screen diagnostic log (no passwords, keystrokes or screen
  content) to help track down any remaining input issues

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
