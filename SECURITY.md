# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Report privately using GitHub's **"Report a vulnerability"** button on the
repository's *Security* tab (GitHub private vulnerability reporting).

Include, if you can:
- affected version and platform (Windows, Linux, Android),
- steps to reproduce,
- impact (what an attacker could do).

We aim to acknowledge reports within 7 days and to agree a disclosure date
with the reporter.

## Supported versions

Only the latest release receives security fixes until version 1.0.

## Security design summary

- Connections use RDP's established security only: **NLA (CredSSP) over TLS 1.2+**.
  Legacy RDP security is refused. No custom cryptography.
- **Server certificates are pinned** (SHA-256 fingerprint) after the user
  approves them on first connection. A changed certificate blocks the
  connection before any credentials are sent. The IronRDP option that accepts
  any certificate is never used (checked in CI by `scripts/check-policy.sh`).
- **Passwords are stored only in the operating system's vault**
  (Windows Credential Manager, Linux Secret Service, Android Keystore),
  never in files, logs or error messages. If no vault exists, the password is
  not saved. Using a saved password needs Windows Hello on Windows, the
  fingerprint/face/screen-lock prompt on Android, and an unlocked keyring on Linux.
- **Logging is capped at `info` in release builds** and the authentication
  library's (`sspi`) logs are always disabled, because at debug level it
  records credential buffers.
- Drive, printer, USB and audio redirection are **off**.
  Clipboard sharing is **opt-in per connection**, plain text only, up to 1 MB,
  in both or one direction. When it is off, the clipboard channel is not offered
  to the host at all. On Android, text from the host is marked sensitive.
- Outside access is intended **only through a VPN** (e.g. WireGuard or
  Tailscale). Never expose RDP port 3389 to the internet.

## Known limitations

- A compromised host can read anything you send through the shared clipboard.
- The first connection to a host relies on you checking the certificate
  fingerprint shown by the app against the host.
- Malware on your own device (for example an Android app with accessibility
  permission) can read the screen.
- On Linux the keyring is usually unlocked when you log in, so saved passwords
  can be used without a new prompt. Don't save passwords there if you want to be
  asked every time.
- Some password copies (inside the RDP library, and on Android inside the app's
  UI layer) cannot be wiped from memory immediately; they are freed when no
  longer used.

## Repository hygiene

This repository must never contain passwords, keys, real hostnames, IP
addresses or personal data. Secret scanning (gitleaks) runs as a pre-commit
hook and in CI. See `CONTRIBUTING.md`.
