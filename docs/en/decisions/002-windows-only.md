---
status: accepted
date: 2026-10-21
topic: platform
---

# Windows only

The program is made for Windows only.

## Question

The program is used on one computer with Windows 11. Should it also run on Linux and macOS?

## Options

| Option | For | Against |
|---|---|---|
| Windows only | Everything needed is built into Windows: hotkeys for the whole system, screen capture, the clipboard, text recognition. One system to test | Doesn't run anywhere else |
| Windows, Linux and macOS | Runs anywhere | Each system captures the screen and takes hotkeys in its own way; on macOS, and on Linux with Wayland, a program needs the user's permission to capture the screen. Several times the work for a program that one person uses on Windows |

## Why

The program is made for one person, and that person works on Windows. Other systems would multiply the work with nobody to use the result.

## Consequences

- The code calls Windows directly, without a layer for other systems.
- The parts that talk to Windows (capture, hotkey, clipboard, browser) are kept apart from drawing and actions, so that they could be replaced if another system is ever needed.
