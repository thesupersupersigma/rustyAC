# Security Policy

rustyAC is a bit-exact Rust rewrite of Assetto Corsa maintained by me in my spare time.

## Supported Versions

Only the **latest release** gets security fixes. Older versions won't be patched; please update.

| Version                  | Supported          |
| ------------------------ | ------------------ |
| Latest release (1.x)     | :white_check_mark: |
| Anything older           | :x:                |

## What's in scope

- `rustyac.exe` and `rustyac-launcher.exe` (the desktop game and AC's launcher as ported by rustyAC)
- The browser build and its page code
- rustyAC's online code: its network protocol handling and the WebSocket gateway used by the browser build
- Anything in this repository that reads files or network data (content, replays, setups, `.ini` files, shared memory)
- The rustyAC website (rustyac.com is in a separate repository).

Examples of what we want to hear about: a crafted car, track, replay or `.ini` file that makes rustyAC run code or write outside its own folders; a malicious server or client that can crash or take over another player's rustyAC; ways the browser build could leak data or reach things it shouldn't.

## What's out of scope

- **Assetto Corsa itself, `acServer`, Content Manager or Custom Shaders Patch.** These aren't rustyAC's code. Report those to Kunos Simulazioni or the respective authors.
- **Cheating in online races** (unless it's a real security hole, like running code on another player's machine).
- **Bugs where rustyAC behaves differently from Assetto Corsa.** Those are normal bugs: open a regular issue.
- Denial-of-service attacks against any server, and social engineering.

## Reporting a Vulnerability

**Please don't open a public issue for security problems.**

Report it privately through GitHub instead:
1. Go to this repository's **Security** tab.
2. Click **Report a vulnerability**.
3. Describe the problem, the version you tested, and steps or files to reproduce it.

What to expect:
- **Acknowledgement within 7 days.**
- **An update at least every 14 days** while it's being worked on.
- If it's **accepted**: a fix in a new release, and credit in the release notes if you'd like it.
- If it's **declined**: an explanation of why (for example, out of scope or not reproducible).

This is a one-person project, so these times are best effort. There's no bug bounty.

Please test only against your own installation and servers you run yourself. Don't test against other people's servers or computers.
