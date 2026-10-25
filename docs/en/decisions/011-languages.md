---
status: accepted
date: 2026-10-25
topic: product
---

# The program's languages

Decided: the program speaks 50 languages, those of most countries: the six official languages of the UN, Arabic, Chinese, English, French, Russian and Spanish, and 44 more, Hindi, a language of its documentation, among them. On its first start it asks which, in a window of tiles with flags: the UN's six first, then every language in the alphabetical order of their own names, the language of Windows marked. Later the settings change it, and "auto" follows the language of Windows. The standard query buttons, the name of a saved file and the comments of the settings file speak the language too.

## Question

ainitShot is published for everyone ([decision](009-the-name-ainitshot.md)), and its windows, hints and messages were in Russian only. Which languages does it speak, how is one chosen, and how are the texts kept?

## Options

| Option | For | Against |
|---|---|---|
| Russian only | Nothing to do | Of no use to most people |
| English only | One language for everyone | Many users read their own language more easily |
| The languages of the documentation, English, Russian and Hindi | Few texts to keep | The program is for everyone |
| The UN's six and Hindi | Most people of the world read one of them | In most countries the program wouldn't speak the country's language |
| The languages of most countries, 50, texts in catalogs, English where a catalog lacks one | In almost every country the program speaks the country's language; another language is one more catalog | Every text is translated into 49 languages |

## Why

Fifty languages cover almost every country, so almost everyone reads the program in their own language. The UN's six alone would leave out most countries, from Germany and Japan to Indonesia and Brazil.

The texts are written in English where they are used, so the code reads as it did. Every other language has a catalog of them: a language added later is a catalog, and a text a catalog lacks shows in English rather than not at all.

## How it works

- **The languages:** English, Chinese, Spanish, Arabic, French, Russian, Hindi, Portuguese, Bengali, Japanese, German, Indonesian, Urdu, Turkish, Vietnamese, Korean, Italian, Persian, Thai, Polish, Ukrainian, Malay, Filipino, Swahili, Amharic, Burmese, Uzbek, Dutch, Romanian, Kazakh, Azerbaijani, Greek, Hungarian, Czech, Swedish, Serbian, Hebrew, Bulgarian, Danish, Finnish, Slovak, Norwegian, Georgian, Croatian, Armenian, Albanian, Lithuanian, Slovenian, Latvian and Estonian.
- **On the first start,** before the presentation "How to use", a window asks for the language. Every language has a tile with its flag, three in a row, and the whole window scrolls.
  - "UN languages" comes first: English, Chinese, Spanish, Arabic, French and Russian, by how many people speak them.
  - "All languages" follows: every language, the UN's six too, in the alphabetical order of their own names.
  - The language of Windows is marked "As in Windows": among the UN's six, or above them if it isn't one of them.
  - Typing the first letters of a language's name goes to its tile.
- **Closing the window** keeps the language of Windows. A settings file from before the program spoke several languages chooses none, so the window asks on the next start too, without the presentation.
- **The setting:** `language` in the settings file is "auto", the language of Windows, or a language's code, such as "en", "de" or "ja". The settings window chooses it in "General".
- **The language of Windows** is the first of the user's languages. One the program doesn't speak gives English.
- **The catalogs** are files in the program, `src/locales/<code>.json`: the program's own texts by their English, and the texts of each window's page by their names. Every text the program shows comes from them:
  - the tray menu, the hints of the toolbars, the notes and notifications;
  - the settings window, the presentation and the invitation to sign in;
  - the strip on Google's pages and the answer card;
  - the comments of the settings file.
- **A new language takes effect at once:** the windows open show it, and the settings file is written with comments in it. Arabic, Hebrew, Persian and Urdu are written from right to left, and the windows' pages turn to them.
- **Query buttons:** a standard button's name and question become those of the new language while they are as they were, whatever else was changed, so the standard "Translate" translates into it. What one wrote oneself stays as it is, and so does the name of a saved file one changed.
- **Google** gets the language too: the address of the AI's answer in the browser and the image search carry it. A few languages go by Google's own codes, such as "zh-CN" for Chinese.

## Consequences

- A text added to the program is added to every catalog. A test finds a text that a catalog lacks, one the program no longer has, and a translation whose placeholders differ from the text's.
- A language is added with its catalog, a line in the program's list of languages and its flag.
- A language is shown with the flag of a country where it is spoken, such as Brazil's for Portuguese and that of the United Arab Emirates for Arabic.
