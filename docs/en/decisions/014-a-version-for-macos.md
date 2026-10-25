---
status: draft
date: 2026-10-25
topic: platform
---

# A version for macOS

Not decided yet: whether ainitShot gets a version for macOS. The program is made for Windows only ([decision](002-windows-only.md)), so a version for the Mac is a port of the program, not a setting of the build. This record weighs the options and the cost.

## Question

Should ainitShot run on macOS too, and what would that take?

## Options

| Option | For | Against |
|---|---|---|
| Windows only, as now | Nothing to do | Mac users stay without the program |
| A port of the Windows part, in Rust, with the rest shared | The same program on the Mac, as fast and as small; drawing, settings, queries, the pages of the windows and the translations stay one | About as much new code as the Windows part has; testing needs a Mac |
| A cross-platform framework (Tauri, Electron) for everything | One code for every system | Screen capture, hotkeys and the frozen screen still need each system's own code; the program becomes heavier and slower |

## What a port takes

About 4,500 lines of the program don't depend on Windows and stay as they are: the selection, the annotations and the magic pencil, the drawing, the toolbars, the queries, the settings and the translations. So do the pages of the windows (settings, presentation, invitation, choice of a language), with a small change of how a page talks to the program.

The Windows part, about 7,700 lines, is written anew for the Mac:

| On Windows | On the Mac |
|---|---|
| Screen capture through GDI | ScreenCaptureKit, after the user allows screen recording |
| The frozen screen, a window over every monitor | A window over every display, above everything |
| The hotkey (RegisterHotKey) | Carbon's RegisterEventHotKey |
| The icon in the notification area | An item in the menu bar |
| The clipboard, the save dialog | NSPasteboard, NSSavePanel |
| Text recognition of Windows | Apple's Vision |
| WebView2 for Google and the windows | WKWebView |
| Text through GDI | Core Text |
| Starting with Windows, notifications | Login item (SMAppService), User Notifications |
| The installer, updates | An application in a `.dmg`, updates from the same `latest.json` |

The risks:

- Google may refuse to sign in inside WKWebView, which it treats as an embedded browser.
- Asking aloud depends on speech recognition in WKWebView.
- Nobody can test the program without a Mac.

## Building and money

- SourceCraft has no macOS workers, neither in its cloud nor self-hosted. The build needs a Mac:
  - GitHub Actions has macOS workers, free for a public repository and limited minutes for a private one;
  - or a Mac of our own as a worker.
- Without Apple's signing and notarization, macOS stops the program on its first start. The Apple Developer Program costs $99 a year.
