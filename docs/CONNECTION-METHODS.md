# Connection methods: which one to use

Cipher Desk can connect to a computer in three ways. You choose the method for
each saved connection (**New connection → Method**). Not sure? Tap **Which method
should I use?** in the form and answer two questions.

| | Private Remote Desktop | Share screen | VNC |
|---|---|---|---|
| **What you see** | Your own Windows desktop | The computer's screen, live | The computer's screen, live |
| **Screen at the computer** | Locked while you work | Stays on, people there see everything | Stays on, people there see everything |
| **The computer needs** | Windows Pro, Enterprise or Education | Cipher Desk (Windows 10/11, any edition) | A VNC program (e.g. TightVNC) |
| **Sign-in** | Windows username + password | Username + password from the Share page | VNC password (max. 8 characters) |
| **Encryption** | Yes (TLS) | Yes (TLS) | No, so only on your own network or Tailscale |
| **Identity check** | Certificate fingerprint | Identity code | None (use Tailscale) |
| **Clipboard** | Yes (if you allow it) | No | No |
| **Default port** | 3389 | 3395 | 5900 |
| **Best for** | Working on your PC privately from anywhere | Helping someone, or when the screen must stay visible | Computers where you can't install Cipher Desk |

Step-by-step tutorials:

- [Private Remote Desktop](methods/PRIVATE-REMOTE-DESKTOP.md)
- [Share screen](methods/SHARE-SCREEN.md)
- [VNC](methods/VNC.md)

## Reaching a computer from somewhere else

All three methods work on the same network (same Wi-Fi or office network). From
anywhere else, use **Tailscale** (free for personal use): install it on both devices
and sign in with the same account. Each device gets an address starting with
`100.`, which you use as the computer address in Cipher Desk. Turn on two-factor
login for your Tailscale account.

Never open ports 3389, 3395 or 5900 on your router. Computers with open
remote-access ports are attacked all the time.
