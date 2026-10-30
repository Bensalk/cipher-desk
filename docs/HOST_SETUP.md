# Setting up the remote computer safely

The app can only protect its own side of the connection. This checklist covers
the computer you connect **to** (SA-4), and how to reach it from outside your
home or office (SA-1). The **Check readiness** item in a connection's menu tests
the parts the app can see.

## 1. Which Windows editions work

Windows **Pro, Enterprise or Education** can accept Remote Desktop connections.
Windows **Home cannot** (it can only connect out). Linux computers need an RDP
server with NLA, for example `xrdp` configured for TLS and NLA, or FreeRDP's server.

## 2. Turn on Remote Desktop with the safe settings

1. *Settings → System → Remote Desktop →* **On**.
2. Keep **"Require devices to use Network Level Authentication"** ticked. The app
   refuses computers without NLA.
3. Note the computer's **certificate fingerprint**, so you can compare it with the one
   the app shows on the first connection. Open **PowerShell as administrator** on that
   computer and paste:

   ```powershell
   $t = (Get-CimInstance -Namespace root/cimv2/TerminalServices -ClassName Win32_TSGeneralSetting -Filter "TerminalName='RDP-tcp'").SSLCertificateSHA1Hash
   $c = Get-Item "Cert:\LocalMachine\Remote Desktop\$t"
   [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($c.RawData)).Replace('-', ':')
   ```

   It prints the SHA-256 fingerprint in the same form as the app (`AB:12:…`). Write it
   down or take a photo. It changes only if Windows creates a new certificate.

## 3. The account needs a password

Remote Desktop only signs in with a **password**: an account with no password is
refused by Windows, and a PIN or fingerprint is not enough. For a local account, add
one under *Settings → Accounts → Sign-in options → Password → Add*. For a Microsoft
account, use your Microsoft password and your email address as the username.
`whoami` in PowerShell shows a local account's exact name (the part after `\`).

## 4. Use a separate account for remote access (S13)

- Create a **standard (non-administrator)** account for remote use, with a long,
  unique password. Add it under *Remote Desktop → Remote Desktop users*.
- Don't use an account whose password you use anywhere else.

## 5. Account lockout

Set an **account lockout policy** so password guessing is stopped:
*Local Security Policy → Account Policies → Account Lockout Policy*, for example
10 attempts, 15-minute lockout. The app itself never retries a wrong password, so
it will not lock you out by accident.

## 6. Keep it updated

Turn on **Windows Update** (security updates install automatically). Remote Desktop
has had serious vulnerabilities that were fixed only by updates.

## 7. Firewall: only from your own network or VPN

- In *Windows Defender Firewall → Advanced settings → Inbound rules*, limit the
  **Remote Desktop** rules to your local network and your VPN's address range.
- **Never forward port 3389 on your router** to the internet. Exposed RDP ports are
  attacked constantly.

## 8. Connecting from outside: use a VPN with two-factor login (SA-1)

To connect from outside your network, first join it through a VPN such as
**WireGuard** or **Tailscale**, with **two-factor login** turned on for the VPN
account. Then connect with the app as if you were at home. Wake-on-LAN usually
does not work through a VPN (it needs the local network).

## 9. Wake-on-LAN (optional)

Turn on *Wake on Magic Packet* in the network adapter's settings (*Device Manager →
network adapter → Advanced* and *Power Management*), and Wake-on-LAN in the
computer's BIOS/UEFI if needed. Enter the adapter's MAC address in the connection.

## 10. Sleep and unattended use

If you want to connect without anyone at the computer, set it not to sleep while
plugged in, or use Wake-on-LAN. Lock the screen or sign out when you finish.
