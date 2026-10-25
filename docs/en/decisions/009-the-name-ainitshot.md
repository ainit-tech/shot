---
status: accepted
date: 2026-10-24
topic: product
---

# The name ainitShot

Decided: the program is called ainitShot. It is to be published as open source by ainit, and its working name, screenshot, can't be found among everything else called so.

## Question

What to call the program now that it is to be published?

## Options (October 2026)

| Option | For | Against |
|---|---|---|
| screenshot, the working name | Says what the program is | A common word, impossible to find; Windows uses it for its own screenshots, and a folder `%APPDATA%\screenshot` may belong to another program |
| Freezeshot | Says what the hotkey does; "-shot" as in Lightshot, which the program replaces; free on GitHub and crates.io | "Freeze" also reads as a program hanging |
| Stopframe or Stopkadr | A freeze-frame, which is what the hotkey makes | Stopframe brings stop-motion animation to mind; Stopkadr means nothing in English |
| AskShot | Says what the program is for | Already the name of a similar program on GitHub, which sends a screenshot to an AI |
| ainitShot | The maker's name with "shot", close to the working name; unique thanks to "ainit" | Without "ainit", "shot" is too common a word to find |

## Why

With the maker's name in it, the name belongs to this program alone and says who publishes it.

## Consequences

- The program calls itself ainitShot: in the titles of its windows, in the hint of the icon in the notification area and in its messages.
- Its folders are `%APPDATA%\ainitShot` for the settings and `%LOCALAPPDATA%\ainitShot` for the profile of the Google window; its entry among the programs that start with Windows is `ainitShot`.
- The Rust package and the program's file are `ainitshot` and `ainitshot.exe`, in small letters, as Rust names packages.
