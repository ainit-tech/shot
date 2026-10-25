---
status: accepted
date: 2026-10-25
topic: product
---

# Masking private data

Decided: the button "Mask" covers the private data of a screenshot with filled rectangles. The text recognition of Windows reads the words and where each one is. Rules find keys, passwords, card and account numbers, e-mail addresses, phone numbers and IP addresses, and OpenAI Privacy Filter, a model running on the computer, finds names, addresses, dates and links too. The program downloads the model by itself in the background and masks with the rules alone until it is downloaded.

## Question

Screenshots go to people and to Google ([decision](008-google-in-the-programs-own-window.md)), and they often carry what shouldn't go: a key in a terminal, a password, a card number, someone's e-mail, name or address. Covering each by hand with a filled rectangle is slow, and something is easily missed. How can the program find it and cover it by itself?

## What is known (October 2026)

- OpenAI Privacy Filter (April 2026, Apache 2.0) marks private data in a text: names, addresses, e-mail addresses, phone numbers, links, dates, account numbers and secrets. Its version with weights of 4 bits is a 917 MB file. With ONNX Runtime 1.31 on the processor it loads in 2.8 seconds, takes about 0.9 GB of memory and marks a screenful of text in 0.2 to 0.8 seconds. On a test text it found names in English and in Russian, addresses, phones, an e-mail, a date, a link, an API key and a card number. Checked on October 25, 2026.
- No ready model hides private data in the picture itself: the open ones that look at pictures have licenses for non-commercial use or none at all, and they were tried on Latin letters only.
- The recognition of Windows gives every word with its rectangle ([decision](010-copying-text-with-windows.md)). The Russian recognition read the digits of a card number as Cyrillic letters, while the English one read them right. Checked on October 25, 2026.
- In drawn DNS records of every kind, as a registrar's table and as dig in a terminal, the recognition read none of their 7 addresses exactly when the picture was made twice as big pixel by pixel: it lost the dots and read zeros as letters. Made twice as big and smoothed, it read 5 and 4 of them; grey, a light ground or three times bigger read no more. It reads no IPv6 address but a short one. The model has no kind for IP addresses: it marked some of them as links and missed others. Checked on October 25, 2026.

## Options

| Option | For | Against |
|---|---|---|
| Rules on the text read | Instant, nothing to download | They don't understand meaning: no names or addresses |
| Rules and OpenAI Privacy Filter | Finds by meaning too; runs on the computer | About 950 MB to download; seconds to load the first time |
| A model that looks at the picture | Would find faces too | Licenses for non-commercial use or none; tried on Latin letters only |

## Why

The recognition with the rectangles of words is already in the program, and a finder that marks parts of a text works for any language the recognition reads. The rules cover what has a fixed shape, at once and without a download; the model adds what takes meaning. Both run on the computer, before the screenshot goes anywhere.

## How it works

- **The button "Mask"** stands on the toolbar between the colour and "Undo"; its key is Ctrl+M.
- **Reading.** The selection with its annotations is read by the recognition of the user's language, and by the English one too when that language isn't written in Latin letters. Each misreads what the other reads right, so both readings are looked at.
- **Finding.** In each reading, the rules and the model mark parts of the text. The words of each part are covered: words next to each other on a line by one rectangle, a little bigger than they are. Covers of the two readings that overlap become one.
- **The rules** find IP addresses of versions 4 and 6 as the recognition reads them: with O, Ø or I for some digits, and a space by a dot or a colon.
- **The covers** are ordinary filled rectangles in the chosen colour: Delete removes one, and Ctrl+Z takes all of them back in one step.
- **The model** is loaded at the first masking and let go after 10 minutes unused.
- **The download:**
  - The model is downloaded in the background into `%LOCALAPPDATA%\ainitShot\models`, from OpenAI's repository at a fixed commit.
  - ONNX Runtime 1.31.0 comes with it. Its one file is taken out of Microsoft's archive by a range of bytes: 6 MB of 83.
  - Every file is checked against its SHA-256 before it takes its place.
  - A download cut short goes on from where it stopped. Without a connection, the program tries again in two minutes.
- **Until the model is downloaded,** masking works with the rules:
  - a thin line under the button's icon shows how far the download has come;
  - the button's hint and a note after masking say that names and addresses aren't looked for yet;
  - when the download ends, one notification says that smart masking is ready.
- **The settings window** has a section "Models" with the model's switch, how its download goes, and a button that pauses it. The switch is `privacy_filter` in the settings file; turning it off removes the model.

## Consequences

- The model takes about 950 MB of the disk, and about 0.9 GB of memory while it is loaded.
- The model is strongest in English: in other languages, especially those not written in Latin letters, it may miss more.
- What the recognition misreads isn't found; small and monospaced letters, as in terminals, are the hardest.
- Faces aren't found: they have no text. A small model of their own can be added later.
- IPv6 addresses aren't hidden: the recognition of Windows doesn't read them. Another recognition, one that reads by letters, would.
- **The masking bench**, a test, draws DNS records of every kind, mixed, masks them, says what was read, found and missed, and saves each picture masked. Pictures of one's own, with the texts they must hide beside them, join it. With `SCREENSHOT_MASKING_KEEP=<folder>` the program saves each masking the same way, for looking into one that went wrong.
