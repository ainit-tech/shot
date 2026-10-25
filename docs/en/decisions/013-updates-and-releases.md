---
status: accepted
date: 2026-10-25
topic: product
---

# Updates, the installer and releases

Decided: ainitShot is installed by an installer of its own and keeps itself up to date. It looks for a new version when it starts and once a day, installs it by itself unless the settings say otherwise, and never goes back to an older one. A release is built by CI from the tag vX.Y.Z and published, signed, at shot.ainit.tech/download.

## Question

The program is published for everyone ([decision](009-the-name-ainitshot.md)). How do people install it, how does it reach a new version, and how is a release made?

## Options

| Option | For | Against |
|---|---|---|
| The program file in a zip, updates by hand | Nothing to build | No Start menu, no autostart, no uninstall; most people would never update |
| Microsoft Store | The Store updates the program | A developer account and the Store's review for every release |
| winget | Installs and updates from the command line | Few people use it, and it still needs an installer |
| An installer of our own, updates checked by the program on our site | Works for everyone without administrator rights; the program chooses a moment for the update when it isn't in the way | We keep the installer, the check and the server |

## Why

The program lives in the tray and is started with Windows, so its users rarely think of updating it; it has to do so itself, quietly and safely. Our own installer and updates work for every user without administrator rights, and the program can install a new version when none of its windows is open. The source repository is private, so the files are served from the program's own server.

## How it works

- **Versions** are X.Y.Z, as in `Cargo.toml`. A release is the tag vX.Y.Z of that version; CI refuses a tag that differs.
- **A release:** on the tag, CI on SourceCraft does the following:
  - builds the program for Windows on a Linux worker (cargo-xwin, target `x86_64-pc-windows-msvc`);
  - makes the installer with Inno Setup 6.7.3 under Wine, the same version every time, checked by its SHA-256;
  - writes `latest.json` with the version and the size and SHA-256 of the installer;
  - signs both with minisign (Ed25519);
  - copies them to https://shot.ainit.tech/download/: the installer to `<version>/ainitShot-<version>-windows-x64-setup.exe`, the version's files first and `latest.json` last.
- **The check:** in the background half a minute after the start, then once a day, and with "Check for updates" in the section "About" of the settings window and in the tray menu. That section shows the version installed and the one available, where people look for them.
- **What may go wrong:** no connection, the site down, a broken or badly signed answer. Every request has a strict time limit. The program waits for the next check and notes what happened in the settings and in its log; nothing else changes.
- **Only forward:** the program takes only a version newer than its own. The signature covers the whole of `latest.json`, which ties the version to the file's size and SHA-256, so an older installer can't pass for a newer one.
- **Installing:**
  - The installer is downloaded to `%LOCALAPPDATA%\ainitShot\updates` and checked.
  - The program waits until none of its windows is open: no frozen screen, settings or conversation.
  - Then it closes as "Exit" does and runs the installer silently, which replaces the program and starts it again.
  - The command never changes, since an installed version runs the installer of the next one with it: `ainitShot-X.Y.Z-windows-x64-setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /UPDATE`. Every installer from 0.2.1 on takes it: it closes the program, replaces it, leaves starting with Windows as it is and starts the new version with `--updated`.
  - After the start, a notification names the new version.
- **Automatically by default:** `autoupdate = true` in the settings file, "Install updates automatically" in the settings window. When it is off, the program only checks and shows; "Update" installs the new version.
- **Only an installed program updates itself.** A program built and started from the source never replaces itself.
- **The installer** is made with Inno Setup, in its dark look, as the program is:
  - its first page has the version, the box "Start with Windows" and "Install"; there is no choice of the folder and no summary;
  - installs for the current user, without administrator rights, into `%LOCALAPPDATA%\Programs\ainitShot`;
  - adds a shortcut to the Start menu and registers the program in Settings → Apps, where it is uninstalled;
  - closes a running ainitShot as "Exit" does before replacing it;
  - its last page says that the program is installed and offers to run it;
  - speaks the language of Windows in every language of the program: Inno Setup's translations, and ours of what it shows in Amharic, Swahili, Filipino and Burmese, which Inno Setup lacks; its own texts come from the program's catalogs, and the build stops if a language lacks one.

  The uninstaller takes starting with Windows away, and the models and updates the program downloaded; it asks whether to remove the settings and the sign-in to Google too.
- **Starting with Windows** is what Windows has: the program's entry in Run, unless the list of startup apps in Task Manager or Settings turns it off. The box of the installer and the switch in the settings are this one setting. A first installation turns it on; installing over the program shows it as it is and keeps it; an update doesn't touch it. The program shows it as Windows has it, and turning it on in the program really turns it on.

## Consequences

- The program and the installer have no Authenticode signature. Windows SmartScreen warns when the installer downloaded from the site is run the first time, and Smart App Control, where it is turned on, may block the program. Updates the program downloads itself don't pass through SmartScreen. Signing needs a paid certificate, or free signing for open-source projects once the repository is public.
- The server, the keys and the delivery from CI are kept in the infra repository; the secret keys exist only in the repository's CI secrets. The public key is built into the program.
- A version for macOS is a question of its own ([decision](014-a-version-for-macos.md)).
