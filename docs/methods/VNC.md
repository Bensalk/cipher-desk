# VNC: step by step

Show and control a computer's screen through a **VNC program** on that computer,
for example when you can't install Cipher Desk there. The screen stays on and
usable at the computer.

Back to [Connection methods](../CONNECTION-METHODS.md).

> **VNC is not encrypted.** Only the password check is protected; the picture and
> everything you type travel in the clear. Cipher Desk therefore connects with VNC
> **only** to addresses on your own network or Tailscale (which encrypts
> everything), and refuses other addresses before sending anything. Prefer
> [Share screen](SHARE-SCREEN.md) on Windows computers where you can install
> Cipher Desk.

---

## On the computer (Windows, with TightVNC)

1. Download **TightVNC** from [tightvnc.com](https://www.tightvnc.com/download.php)
   (the 64-bit installer) and install it. Choose **Custom** and install only
   **TightVNC Server** (the viewer isn't needed).
2. When asked for passwords, set a **password for remote access**. VNC uses only
   the **first 8 characters**, so use 8 random letters and digits. Also set an
   **administrative password**, so nobody can change the settings.
3. When Windows asks about the firewall, allow **Private networks** only.
4. Set your network to **Private**: *Settings → Network & internet →* your
   connection *→ Network profile type → Private.*
5. **To connect from anywhere:** install [Tailscale](https://tailscale.com/download)
   and sign in. Note the computer's Tailscale address (starting with `100.`), shown
   in the Tailscale app. On the same Wi-Fi you can use the normal address
   (`ipconfig`, the *IPv4 Address* line).
6. Optional, recommended: in the TightVNC Server settings (tray icon →
   *Configuration → Access Control*), allow only your network and Tailscale
   addresses (`100.64.0.0` – `100.127.255.255`).

**Linux:** use a VNC server such as `x11vnc`, with a VNC password
(`x11vnc -storepasswd`), reachable from your network or Tailscale only.

## On your phone or other computer

1. Install Cipher Desk and, to connect from elsewhere, **Tailscale** with the same
   account.
2. **New connection.** Method: **VNC**. There is no username.
3. Computer: the address from step 5. Port: leave empty (5900).
4. Optional: **Save password**.
5. **Connect.**

VNC has no certificate, so Cipher Desk can't check that it reached the right
computer. Tailscale does this for you, which is another reason to use it.

## Good to know

- **Keyboard:** keys are sent for a US keyboard layout. Text typed with the
  phone's keyboard (or **Type text**) is sent as characters, so accents work.
- **Clipboard sharing** is not available with VNC.
- **Speed:** Cipher Desk uses compressed updates (Tight and ZRLE) when the VNC
  program offers them.

## If it doesn't work

| Message | What to do |
|---|---|
| *VNC works only on your own network* | The address is on the internet. Use the computer's local address or its Tailscale `100.` address. |
| *The computer does not answer on port 5900* | Is TightVNC Server running (tray icon)? Is the network **Private** and the firewall allowing it? Is Tailscale connected on both devices? |
| *Wrong username or password* | Check the VNC password (only the first 8 characters count). After several wrong tries, TightVNC blocks new attempts for a while. |
| *Secure connection not supported* | The VNC program asks for a sign-in method Cipher Desk doesn't support. Turn on plain VNC password authentication in it. |
