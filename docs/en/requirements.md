---
status: accepted
note: Describes what the program does; the program is the source of truth, and this page follows it.
---

# Requirements

ainitShot is a small program made for its author's everyday work and published as open source by ainit ([decision](decisions/009-the-name-ainitshot.md)). By a hotkey it takes a part of the screen, lets you draw and write on it, and then copies it or its text, saves it, asks Google's AI mode about it or finds where it comes from. It replaces Lightshot: it adds what Lightshot lacks and leaves out what is not used. It is not a product for sale ([why](decisions/001-own-program.md)).

## How it works

1. **The hotkey** freezes the screen: Print Screen, unless one chooses other keys on the first start or later in the settings. The program suggests Alt with the key left of 1 in any keyboard layout (Ё in Russian, ^ in German), for a screenshot with the left hand alone.
2. The mouse draws the selection, as in Lightshot; the screen around it is dimmed. While nothing is selected, the window under the mouse is lit up, and a click without dragging selects it: the window stays lit until the mouse goes a few pixels away, and from there on the mouse draws the selection. Ctrl+A selects the monitor under the mouse. The selection can then be moved and resized by its edges. The hotkey pressed again takes the selection away and leaves the same frozen screen: the windows light up again, and a smaller part can be selected even inside a window selected whole; Ctrl+Z brings the selection back with its annotations.
3. Two toolbars appear next to the selection, as in Lightshot: the tools for drawing in a row under it at its right end, and the actions, the query buttons, masking, copying and saving, in a column to the right of it at its bottom. The settings put either toolbar on any side of the selection, pressed to either end of that side: above and below the selection a toolbar is a row, beside it a column, and two toolbars pressed to the same end of one side stand in a row in the order chosen. A toolbar with no room on its side goes to the opposite one, or inside the selection's edge.
4. Tools draw annotations on the selection, and any annotation can be put right afterwards (see below); Ctrl+Z undoes the last change, Ctrl+Y brings it back.
5. An action finishes the work: copy the picture or its text, save it, or a query. Esc closes everything and does nothing; a right click starts over, as the hotkey does, and a second one closes.

The keys work in any keyboard layout. A letter is the key with that Latin letter on it or, in a layout without Latin letters, the key where English has it: in Russian the text tool's N is the key with Т. An input method for Chinese, Japanese or Korean leaves the letters to the program, except while text is being written.

## Design

Minimal and modern. Above all, it must be convenient: during development every part is thought through from the user's point of view.

Nothing is on the screen but the selection and its toolbars; icons with hints instead of words; the look of Windows 11: rounded corners, soft shadows, smooth lines.

The program speaks 50 languages, those of most countries: the six official languages of the UN, Arabic, Chinese, English, French, Russian and Spanish, and 44 more, from German and Japanese to Swahili. On its first start it asks which, with a tile and a flag for each language, the UN's six first; later the settings change it ([decision](decisions/011-languages.md)).

On the program's first start, once the language is chosen, a window asks for the keys of a screenshot. Print Screen comes chosen; other keys pressed take its place, and the program says at once whether another program has them. Where Windows opens its own screen capture with Print Screen, the window says so and turns that off on a click. A quiet tip suggests Alt with the key left of 1, named as the keyboard labels it.

Then, and later from the menu of the icon in the notification area, "How to use" plays a clip of the program at work, the one the site shows: short chapters on taking a screenshot, the magic pencil, hiding and masking what is private, putting annotations right, copying the text, saving, the queries to Google, aloud too, the languages and the settings. The clip speaks the program's language and shows the hotkey and the query buttons from the settings; the list of its chapters and the arrow keys go to any of them. Under the clip the window offers to sign in to Google while nobody is signed in, and opens the settings.

The menu of the icon in the notification area takes a screenshot and opens the settings, "How to use" and the site's page where people support the program, in the program's language where the site speaks it; while updates are installed by hand, it offers a newer version too. The menu is dark or light, as Windows is.

## Tools

| Tool | Key | What it draws | Compared with Lightshot |
|---|---|---|---|
| Pencil | P | A thin line exactly along the mouse | The same |
| Brush | B | A wide translucent line by hand, like a highlighter, for marking text | The same as Lightshot's marker |
| Line | L | A straight line | The same |
| Arrow | A | A straight line with an arrowhead | The same |
| Rectangle | R | A frame | The same |
| Filled rectangle | F | A solid rectangle that hides what is under it | New |
| Text | N | A label written where the selection is clicked, with a soft shadow that keeps it readable on any background | Lightshot has text, without the shadow |
| Magic pencil | M | A rough loop around something becomes a neat outline | New |

All tools share the colour and the line width. The colours are in the settings, red first; the mouse wheel changes the width, and the size of text with it. Shift keeps lines at multiples of 45°, makes rectangles square and keeps the magic pencil's loop as drawn. What the magic pencil draws exactly is a separate [decision](decisions/006-magic-pencil.md).

Text is written as typed: Enter puts it down, Shift+Enter starts a new line, Ctrl+V pastes, the arrow keys, Home and End move the caret, Delete takes away the letter after it, Esc lets the text go. While text is being written, the keys of the tools and buttons type letters.

**Masking**, with its button between the colour and Undo or with Ctrl+M, covers the private data of the screenshot with filled rectangles of the colour chosen: keys, passwords, card and account numbers, e-mail addresses, phone numbers and IP addresses, and with OpenAI Privacy Filter, a model that runs on the computer and that the program downloads by itself, names, addresses, dates and links too. Ctrl+Z takes all the rectangles back at once ([decision](decisions/015-masking-private-data.md)).

## Changing annotations

What is drawn can be put right until the screenshot leaves:

- The annotation just drawn is picked: a line and an arrow show round handles at their ends, a rectangle at its corners, other annotations a thin dashed frame. A handle is dragged with any tool chosen, so the tip of an arrow is put right at once; Shift keeps the line at 45° and the rectangle square.
- With no tool chosen (a second click on the chosen tool puts it away), any annotation is dragged to move it, and a click picks it. With a tool chosen, a click on an annotation that draws nothing picks it.
- Delete or Backspace removes the annotation picked.
- A double click on a label rewrites it, whatever the tool; with the text tool one click is enough, and a drag moves the label. Esc brings the label back as it was.
- Each change, whether a move, a new shape, a rewrite or a removal, is one step of Ctrl+Z.

The mouse shows what it would do: a hand over a handle, a cross of arrows over an annotation that moves, the text cursor over a label to rewrite.

## Actions

| Action | Keys | What it does |
|---|---|---|
| Copy | Ctrl+C, Enter | Puts the screenshot into the clipboard |
| Copy text | Ctrl+Shift+C | Reads the text in the screenshot with the text recognition built into Windows, on the computer, and puts it into the clipboard ([decision](decisions/010-copying-text-with-windows.md)) |
| Save | Ctrl+S | Saves the screenshot to a PNG file in the folder from the settings and puts it into the clipboard too; Ctrl+Shift+S asks where |
| Ask Google | G | A query to Google's AI mode: who or what is in the picture; the conversation comes out next to the selection, to ask further; held, it asks one's own question aloud |
| Translate | T | A query to Google's AI mode: translate the text in the picture; the translation alone shows in a card next to the selection |
| Search by image in Google | I | An image search in Google: the picture alone, to find where it comes from and where else it is; the exact matches, the pages with this very picture, come out next to the selection |

Every action takes the screenshot with its annotations: text hidden under a filled rectangle stays hidden, and a loop drawn with the magic pencil shows the AI mode what to look at.

The query buttons are of two kinds and stand in two groups on their toolbar ([decision](decisions/004-one-kind-of-query.md)). The AI mode takes one picture with a question and answers in words: what is in it, what it means, how to translate it. An image search takes the picture alone and finds where it comes from and where else it is: the AI is weak at that, and a question would narrow the search. The buttons that ask the AI mode differ in the mini prompt and in where the answer opens. The buttons, their mini prompts and keys are in the settings.

How the screenshot reaches Google is a separate [decision](decisions/005-how-the-picture-reaches-a-search-engine.md). Google's pages open in the program's own window, where the user signs in to Google once, as the program offers to when nobody is signed in, and the program puts the screenshot into them itself: "Ask Google" and "Translate" into AI Mode with the question, and "Search by image in Google" into Google Images, with its results on the tab of exact matches ([decision](decisions/008-google-in-the-programs-own-window.md)).

Saving puts the file straight into the folder from the settings, and the picture into the clipboard as well, ready to paste, unless the settings say otherwise; a notification says where the file is, and a click on it shows the file in its folder. The folder is Windows' own folder of screenshots by default, "Pictures\Screenshots", where Win+PrintScreen saves too; it is inside OneDrive when OneDrive backs up the Pictures folder. Ctrl+Shift+S asks where to save, starting in that folder, and the settings can have the button and Ctrl+S ask as well. A file is named by the pattern from the settings, by default "Screenshot {date} {time}" in the language of the program. Only the screenshots saved this way are kept.

## What is left out

Whatever Lightshot has and this page doesn't: uploading to prnt.sc with public short links, sharing to Facebook, Twitter, Pinterest and VK, printing; and whatever doesn't serve the steps above: video recording, capturing a scrolling page, accounts, a cloud. Blurring is left out too: a filled rectangle hides more reliably, since blurred and pixelated text can sometimes be restored, and screenshots are sent to Google.

## Where it runs

Only on Windows ([why](decisions/002-windows-only.md)). The program is written in Rust and waits in the background for the hotkey ([why](decisions/003-rust.md)). The settings are in a text file, changed in a settings window with sections and a search ([decision](decisions/007-settings-in-a-file.md)); its section "About" shows the version and where updates stand, invites to support the program, and leads to the site, the source code, the license and the open source components with theirs ([decision](decisions/012-license.md)). An installer puts it in place for the current user, without administrator rights, and has it start with Windows; then the program keeps itself up to date, installing a newer version by itself unless the settings say otherwise and never going back to an older one ([decision](decisions/013-updates-and-releases.md)).
