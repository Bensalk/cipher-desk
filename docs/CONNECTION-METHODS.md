# Connection methods: which one to use

Cipher Desk can connect to a computer in three ways. You choose the method for
each saved connection (**New connection → Method**). Not sure? Tap **Which method
should I use?** in the form and answer two questions.

| | Share screen | Private Remote Desktop | VNC |
|---|---|---|---|
| **What you see** | The computer's screen, live | Your own Windows desktop | The computer's screen, live |
| **Screen at the computer** | Stays on, people there see everything | Locked while you work | Stays on, people there see everything |
| **The computer needs** | Cipher Desk (Windows 10/11, any edition) | Windows Pro, Enterprise or Education | A VNC program (e.g. TightVNC) |
| **Sign-in** | Username + password from the Share page | Windows username + password | VNC password (max. 8 characters) |
| **Reach the sign-in screen** | Only with sign-in screen access turned on (a background service) | Yes | No |
| **Encryption** | Yes (TLS) | Yes (TLS) | No, so only on your own network or Tailscale |
| **Identity check** | Identity code | Certificate fingerprint | None (use Tailscale) |
| **Clipboard** | No | Yes (if you allow it) | No |
| **File transfer** | Both ways, by copy-paste; no size limit | One-way, to the remote only; 256 MB, one at a time; Windows hosts only | None |
| **Default port** | 3395 | 3389 | 5900 |
| **Best for** | Helping someone, or when the screen must stay visible | Working on your PC privately from anywhere | Computers where you can't install Cipher Desk |

Step-by-step tutorials:

- [Share screen](methods/SHARE-SCREEN.md)
- [Private Remote Desktop](methods/PRIVATE-REMOTE-DESKTOP.md)
- [VNC](methods/VNC.md)

## Reaching a computer from somewhere else

All three methods work on the same network (same Wi-Fi or office network). From
anywhere else, use **Tailscale** (free for personal use): install it on both devices
and sign in with the same account. Each device gets an address starting with
`100.`, which you use as the computer address in Cipher Desk. Turn on two-factor
login for your Tailscale account.

Never open ports 3389, 3395 or 5900 on your router. Computers with open
remote-access ports are attacked all the time.
