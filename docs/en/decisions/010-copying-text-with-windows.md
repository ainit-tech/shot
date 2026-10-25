---
status: accepted
date: 2026-10-25
topic: platform
---

# Copying the text of a screenshot with the recognition of Windows

Decided: "Copy text" reads the text in the screenshot with the text recognition built into Windows, on the computer, and puts it into the clipboard. It reads with the recognition of the first of the user's languages that isn't English and, when that language isn't written in Latin letters, takes the lines in English from the English one.

## Question

The text in a screenshot is often needed as text: an error in a dialog, words in a video or a picture, text in a window that can't be selected. How does the program get it?

## What is known (October 2026)

- Windows has text recognition built in (`Windows.Media.Ocr`): it works without the internet, for the languages whose recognition Windows has, usually those of the user's languages. On the computer it was made for, it has English and Russian. Checked on October 25, 2026.
- Windows picks the recognition of the first of the user's languages. With English first, a line in Russian came out as Latin letters. The Russian recognition read the Russian line exactly but an English one with mistakes: "Те" for "the", a capital I for an l. Checked on October 25, 2026.
- A screenshot with a line in English and a line in Russian, read by both recognitions, each line taken from the one whose letters it is written in, came out exactly, in 0.04 seconds. Checked on October 25, 2026.

## Options

| Option | For | Against |
|---|---|---|
| The recognition of Windows | Instant; works without the internet; the screenshot stays on the computer; nothing to install | Only the languages whose recognition Windows has; one language per reading |
| A query to Google's AI with a mini prompt such as "Write out the text" | Reads any language and layout | Takes seconds and the internet; the screenshot goes to Google; the AI may change words |
| Tesseract, an open recognition, inside the program | Many languages | Tens of megabytes of models; the program is no longer small |

## Why

Copying text is quick and often private: it should be instant and stay on the computer. The recognition of Windows does that and costs nothing to install. A query to Google stays for what it reads better, such as handwriting, as a query button of one's own.

## How it works

- The button "Copy text" on the toolbar and Ctrl+Shift+C read the screenshot with its annotations, so that text hidden under a filled rectangle stays hidden. Meanwhile the toolbar says that it is reading the text.
- A picture that fits the recognition twice as big is made twice as big and smoothed, since small letters of the screen are read better bigger, and dots and zeros come out right only smoothed ([decision](015-masking-private-data.md)); one too big for it is made smaller.
- The lines go into the clipboard one under another, and the frozen screen closes. If there is no text, the toolbar says so and the screen stays.

## Consequences

- The languages that can be read are those whose recognition is installed in Windows: a language added in the settings of Windows brings its recognition.
- When the first language that isn't English is written in Latin letters, its recognition reads English too, and only it is used.
