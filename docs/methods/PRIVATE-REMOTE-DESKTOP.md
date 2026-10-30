# Private Remote Desktop: step by step

Connect to a Windows computer with Windows' own Remote Desktop. You get your
desktop on your phone or another computer, and the computer's own screen **locks**
while you work, so nobody there can see what you do.

Back to [Connection methods](../CONNECTION-METHODS.md).

**You need:** a Windows **Pro, Enterprise or Education** computer (Windows Home can't
accept Remote Desktop: use [Share screen](SHARE-SCREEN.md) instead), and an account
on it **with a password**.

---

## On the computer (once)

1. **Check the edition.** *Settings → System → About → Edition.*
2. **Give the account a password**, if it has none. *Settings → Accounts →
   Sign-in options → Password → Add.* A PIN or fingerprint is not enough.
3. **Find the username.** Open PowerShell and type `whoami`. Use the part after
   the `\` (for example `bensal`). With a Microsoft account, use your email address.
4. **Turn on Remote Desktop.** *Settings → System → Remote Desktop → On.* Keep
   **"Require devices to use Network Level Authentication"** ticked.
5. **Set your network to Private.** *Settings → Network & internet →* your
   connection *→ Network profile type → Private.* On a Public network, Windows
   blocks Remote Desktop.
6. **Write down the certificate fingerprint**, to check it on the first
   connection. Open **PowerShell as administrator** and paste:

   ```powershell
   $t = (Get-CimInstance -Namespace root/cimv2/TerminalServices -ClassName Win32_TSGeneralSetting -Filter "TerminalName='RDP-tcp'").SSLCertificateSHA1Hash
   $c = Get-Item "Cert:\LocalMachine\Remote Desktop\$t"
   [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($c.RawData)).Replace('-', ':')
   ```

7. **To connect from anywhere:** install [Tailscale](https://tailscale.com/download)
   and sign in. Find the computer's Tailscale address in the Tailscale app, or in
   PowerShell:

   ```powershell
   (Get-NetIPAddress -InterfaceAlias "Tailscale" -AddressFamily IPv4).IPAddress
   ```

   It starts with `100.`. On the same Wi-Fi you can use the normal address instead
   (`ipconfig`, the *IPv4 Address* line).
8. **For unattended use:** *Settings → System → Power →* set sleep to **Never**
   while plugged in.

More safety settings (separate account, account lockout, firewall):
[HOST_SETUP.md](../HOST_SETUP.md).

## On your phone or other computer

1. Install Cipher Desk ([download](../../README.md#download)) and, to connect from
   elsewhere, **Tailscale** with the same account.
2. **New connection.** Method: **Private Remote Desktop**.
3. Computer: the address from step 7. Port: leave empty (3389). Username: from
   step 3.
4. Optional: tick **Save password** so you don't have to type it each time (it is
   kept in the phone's secure storage).
5. **Save**, then open the connection's menu → **Check readiness**. Every line
   should be green.
6. **Connect.** The first time, compare the fingerprint with the one from step 6.
   Tap **They match — trust** only if they do.

## If it doesn't work

| Message | What to do |
|---|---|
| *The computer does not answer on port 3389* | Is Remote Desktop on (step 4)? Is the network **Private** (step 5)? Is Tailscale connected on both devices? |
| *Wrong username or password* | Use the name from `whoami` (part after `\`) and the account's **password**, not the PIN. |
| *Remote Desktop isn't available* | The computer runs Windows Home. Use [Share screen](SHARE-SCREEN.md). |
| *Secure connection not supported* | Turn on Network Level Authentication (step 4). |
| The screen at the computer goes to the lock screen | That is how Private Remote Desktop works. To keep it visible, use [Share screen](SHARE-SCREEN.md). |
