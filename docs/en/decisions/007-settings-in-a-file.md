---
status: accepted
date: 2026-10-21
topic: platform
---

# Settings in a text file, with a window over it

All settings live in one text file, `settings.toml`, and a settings window changes them. The window has sections and a search over all settings; a change takes effect at once and goes into the file with its comments, so the file can still be edited by hand.

## Question

Where to keep the hotkey, the colours, the buttons with their mini prompts and the folder for files, and how to change them?

## Options

| Option | For | Against |
|---|---|---|
| A settings window alone | Familiar; nothing to learn | The settings are hidden in the program: hard to copy to another computer or to keep in Git |
| A text file alone | Little code; easy to edit, to copy and to keep in Git; comments in the file explain every setting | Editing by hand is slow and error-prone: a hotkey is typed by its name, a colour as "#RRGGBB", and a key is checked against the tools' keys by eye |
| A text file with a window over it | The window is convenient and checks what it changes; the file stays easy to read, copy and edit | Two ways to change a setting, which must agree |
| The Windows registry | The usual place for settings | Hard to see and to edit |

## Why

A text file with comments is easy to read, to copy and to keep, and the window spares editing it by hand: a hotkey is pressed rather than named, a colour is picked, and a key that is taken is refused at once. Both change the same file, so they always agree.

## How it works

- The file is `%APPDATA%\ainitShot\settings.toml`. On the first start the program writes it with the default values and a comment for every setting.
- "Settings" in the menu of the icon in the notification area opens the settings window, in the middle of the monitor under the mouse. It is a page in WebView2, made with the engine that the program's Google window has started ([decision](008-google-in-the-programs-own-window.md)), and dark like the rest of the program.
- On the window's left are a search and the sections: General (the hotkey, the language, starting with Windows, the file itself), Drawing (the colours and the width of the line), Panels (where the two toolbars stand: eight places around the selection for each, with a picture of both), Saving (asking where to save, copying to the clipboard too, the folder, the file's name), Query buttons, and Google account.
- The search looks through the names and descriptions of all settings, and words that mean them, in all sections at once. It shows the settings found under the names of their sections, with the words marked, and how many each section has; the settings found can be changed right there. Ctrl+F goes to the search; Enter opens the first setting found in its section.
- A change takes effect as soon as the window has checked it: a switch or a colour at once, typing a moment after the last key. The window writes the whole file anew, with its comments, first into a file beside it, so that a half-written file is never read. A setting changed from its default has a button that brings the default back; a removed colour or button comes back with "Undo".
- The hotkey is recorded: the window listens for the keys pressed, while the program holds the old hotkey back so that it doesn't freeze the screen. A letter, a digit or another key that would get in the way of typing needs Ctrl, Alt or Win. A hotkey that another program has taken is refused, and the old one stays. Keys are shown as the keyboard labels them: PrtSc, and Ё for the key left of 1 on a Russian keyboard.
- On the first start a window of its own asks for the keys, with Print Screen chosen; a settings file keeps the keys it has. `ainitshot.exe --first-run` shows the windows of the first start again, in the copy running if there is one, and changes nothing unless one chooses there.
- Colours are picked with the colour picker and put in order by dragging; the first one is chosen at once. The width has a slider and a stroke of the first colour at that width. The folder is chosen with the dialog of Windows, and the file's name shows what it becomes now.
- Each query button is a card with its icon, its name, what it does and its key; a click opens the card to change it: the name, where the screenshot goes, in plain words, the question to the AI, the key and the icon. A key is pressed rather than typed, and refused if a tool or another button has it. Cards are put in order by dragging.
- What can't be saved, an empty name for one, is shown under its field, and nothing is saved until it is put right.
- The program rereads the file when it changes. If the file has a mistake, the program says where and keeps the previous settings; the open window says so too, and a setting changed in it writes the file anew.
- The window closes once it has saved what is still being typed. Without WebView2, "Settings" opens the file in a text editor.

## Consequences

- A key belongs to one button and never to a tool: the program refuses a file where it doesn't, as it refuses one with a mistake.
- The settings window depends on WebView2, as the Google window does. Its page is in the program itself; the window is made when opened and let go when closed.
