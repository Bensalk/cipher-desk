# Share screen: step by step

Show and control a Windows computer's screen **while it stays on and usable** at
the computer, like AnyDesk or TeamViewer. Cipher Desk on the computer shares its
screen; Cipher Desk on your phone or another computer connects to it.

Back to [Connection methods](../CONNECTION-METHODS.md).

**You need:** Cipher Desk on a Windows 10 or 11 computer (any edition, Home too)
and Cipher Desk on the device you connect from.

> People at the computer see everything you do, and you see everything on its
> screen. Use [Private Remote Desktop](PRIVATE-REMOTE-DESKTOP.md) when the screen
> should stay private.

---

## On the computer you want to share (once)

1. Install Cipher Desk ([download](../../README.md#download)) and open it.
2. Open the **Share screen** tab (left side, or at the bottom on small windows).
3. A strong **password** is created for you and kept in Windows' secure password
   storage. Tap the eye icon to see it. The refresh icon makes a new one (devices
   using the old one must then use the new one).
4. Optional: change the **Username for viewers** (any name; it is not your Windows
   account) and the **Port** (3395). Tap **Save**.
5. Tap **Start sharing**. The first time, Windows asks whether Cipher Desk may use
   the network: allow **Private networks** only.
6. Note what the page shows under *To connect*: the **computer address**, port,
   username, password and **identity code**.
7. **To connect from anywhere:** install [Tailscale](https://tailscale.com/download)
   and sign in. The page then also lists the Tailscale address (starting with `100.`).
8. **For a computer you want to reach when nobody is there** (for example an
   office PC):
   - turn on **Start sharing when Cipher Desk opens** and **Open Cipher Desk when I
     sign in to Windows**;
   - set *Settings → System → Power →* sleep to **Never** while plugged in;
   - set *Settings → Accounts → Sign-in options →* "If you've been away, when should
     Windows require you to sign in again?" to **Never**, and turn off any screen
     saver that locks. Windows hides the lock screen from sharing programs, so a
     locked computer can't be used remotely.

   A computer that stays signed in can be used by anyone standing at it. Only do
   this where that is acceptable, and lock it yourself (Windows key + L) when you
   need to.

## On your phone or other computer

1. Install Cipher Desk and, to connect from elsewhere, **Tailscale** with the same
   account.
2. **New connection.** Method: **Share screen**.
3. Computer: the address from step 6 (or the `100.` address). Port: leave empty
   (3395) unless you changed it. Username and password: from the Share page.
4. Optional: **Save password**.
5. **Connect.** The first time, compare the identity code with the one on the
   Share page. Tap **They match — trust** only if they do.

While someone is connected, the computer shows **"Your screen is being shared"**
with a **Stop** button at the top of Cipher Desk.

## Good to know

- **Security:** the connection is encrypted (TLS) and needs the password. After 5
  wrong passwords, new connections are refused for 15 minutes.
- **Cipher Desk's own window is hidden** from the person viewing (it looks black or
  is left out), so they can never read the sharing password from it.
- **What can't be controlled remotely:** the Windows sign-in and lock screen (so you
  can't type the Windows password through Share screen), Ctrl+Alt+Del, and "Do you
  want to allow this app…" (administrator) prompts. Windows protects these from
  sharing programs that run as a normal user, as Cipher Desk does.
- **Programs running as administrator** (for example Task Manager or an installer)
  can be seen but not clicked, unless Cipher Desk itself is started with *Run as
  administrator*.
- **"Open Cipher Desk when I sign in"** starts it after you sign in to Windows, not
  before. After a restart, someone must sign in at the computer first, or use
  Private Remote Desktop (which asks for the Windows password) — unless you turn on
  **sign-in screen access** below.

## Signing in from home (the sign-in screen)

Normally Share screen can only show the screen once someone is already signed in.
To sign in to the computer from another device — at the Windows **sign-in or lock
screen** — turn on **sign-in screen access** on the Share screen page.

1. On the computer, Share screen page → **Turn on sign-in screen access**.
2. Approve the one-time **Windows administrator** prompt. This installs a small
   background service ("Cipher Desk Host") that starts with Windows.
3. The page then shows the username, port and a **separate password** for sign-in
   screen sharing. Use those on the other device (method **Share screen**).

With this on, you can connect to the computer even at the lock screen or just
after it restarts, type the Windows password, and keep working with the screen
shared. To turn it off, use **Turn off sign-in screen access** on the same page
(also a one-time admin prompt).

Notes:
- It needs administrator approval **once** to install and once to remove, because a
  background service is a system-wide change. Day to day it needs nothing.
- Its password is kept encrypted on that computer and is different from the ordinary
  Share screen password.
- Programs running as administrator still can't be clicked unless Cipher Desk itself
  was started as administrator; everything else, including the sign-in screen, works.
- Turn it off from the app **before uninstalling Cipher Desk** (a normal uninstall
  can't remove a system service).
- **Clipboard sharing** is not available with Share screen.
- **Main screen only:** with several monitors, the main one is shared.
- **Stop sharing** at any time with the Stop button on the Share page or in the
  banner. Closing Cipher Desk also stops it.

## If it doesn't work

| Message | What to do |
|---|---|
| *The computer does not answer on port 3395* | Is sharing **on** (Share page says "waiting for a viewer")? Is the network **Private**? Did you allow Cipher Desk in the Windows firewall? Is Tailscale connected on both devices? |
| *Wrong username or password* | Type them exactly as on the Share page (the password has dashes). |
| *Sharing paused after wrong passwords* | Wait 15 minutes, or stop and start sharing. If you didn't type wrong passwords yourself, make a new password. |
| *The sharing port is already in use* | Another program uses 3395. Choose another port on the Share page and use it on the phone too. |
| The picture freezes on a lock screen | The computer locked. Sign in at the computer, and see step 8. |

To allow Cipher Desk in the firewall later: *Windows Security → Firewall & network
protection → Allow an app through firewall → Cipher Desk → Private*.
