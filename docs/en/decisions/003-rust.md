---
status: accepted
date: 2026-10-21
topic: platform
---

# Rust and the Windows API

The program is written in Rust, calls the Windows API directly and is one small executable that waits in the background for the hotkey.

## Question

What to write the program in, so that the screen freezes at once after the hotkey and the program stays small?

## Options

| Option | For | Against |
|---|---|---|
| Rust with the Windows API | One small file without a runtime; starts and draws fast; the documentation engine of this repository is in Rust too | The selection, the toolbar and the drawing are made by hand: more code than with a ready interface framework |
| C# and .NET (WinForms or WPF) | The usual choice for such programs: ShareX and Greenshot are written in C#; an interface is quick to make | Needs the .NET runtime, or a self-contained build of tens of megabytes |
| Electron or Tauri | The interface in HTML and CSS | A browser engine is heavy (Electron) or awkward for a full-screen frozen screen with drawing; the hotkey and capture still need native code |
| AutoHotkey | Hotkeys in a few lines | Not suited to an editor with drawing and undo |

## Why

The program stays in the background all the time and must react at once, so a small native file without a runtime suits it best. Rust gives that, and this repository already uses it. The extra work on the interface is limited: one full-screen window with a selection, a toolbar and a handful of tools.

## Consequences

- One process in the background, started with Windows if the settings say so, with an icon in the notification area that takes a screenshot, opens the settings and quits.
- The screen is captured on all monitors, with their scaling, and the full-screen window covers them all.
