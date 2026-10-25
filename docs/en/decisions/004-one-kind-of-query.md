---
status: accepted
date: 2026-10-21
topic: queries
---

# One kind of query, to the AI mode or the image search of a search engine

Every query button is one action, a query: it sends the screenshot to Google and opens the answer in the browser or in the program's own window. Each button is an entry in the settings: a name, where the screenshot goes and a mini prompt. The buttons are of two kinds and stand in two groups on a toolbar of their own: those that ask the AI mode a question about the picture, and those that search by the picture alone in the image search.

## Question

The buttons do almost the same: they hand the picture to a search engine and ask it something. Should they be separate actions, or one? And where in the search engine should the picture go?

## Options

| Option | For | Against |
|---|---|---|
| Separate actions | Each can be tuned in its own way | Several times the code; a new button needs new code |
| One query, the buttons are entries in the settings | A new button, such as "Explain" or "Where to buy", is one entry in the settings, with no code | Every button has to fit one scheme: picture, where it goes, mini prompt |

| Where the picture goes | For | Against |
|---|---|---|
| The AI mode: the answer on top of Google's results and its AI Mode | Answers a question about the picture in words: explains it, translates it | The answer can be wrong, as with any AI; weak at finding where a picture comes from |
| The image search: Google Lens | Finds where the picture comes from and where else it is | Doesn't answer a question: no translation, no explanation; a question would narrow the search |

## Why

The buttons differ only in where the picture goes and what is asked about it. That is data, not code. The two places do different jobs: the AI mode answers a question about the picture in words, and the image search finds where the picture comes from and where else it is, which the AI does poorly. One button that mixes them does neither well: a question narrows the image search, and the AI may invent a source. So each button does one of the two, and the toolbar shows them as two groups.

## How it works

- The program knows how to reach Google's AI mode and image search. How exactly is a separate [decision](005-how-the-picture-reaches-a-search-engine.md).
- A button is a name, where the screenshot goes and, for the AI mode, a mini prompt. The first buttons:

| Button | Where | Mini prompt |
|---|---|---|
| Ask Google | AI mode: AI Mode in the program's window next to the selection, to ask further | Who or what is in the picture? Explain. |
| Translate | AI mode: AI Mode in the program's hidden window, the answer in a card next to the selection | Translate the text in the picture into Russian. Answer with the translation only. |
| Search by image in Google | Image search: the exact matches in the program's window next to the selection | None |

- The query buttons stand on a toolbar of their own, by default a column to the right of the selection at its bottom, which the settings move to any side (see the [requirements](../requirements.md)): those that ask the AI mode come first, and the image search buttons after them as a group of their own.
- A button for a conversation (`mode = "chat"`), held, asks the user's own question aloud in place of its mini prompt ([decision](008-google-in-the-programs-own-window.md)).
- A query sends the screenshot with its annotations. Then a loop drawn with the magic pencil shows the AI mode what to look at, and the mini prompt can ask about "what is circled".

## Consequences

- Mini prompts are edited in the settings ([decision](007-settings-in-a-file.md)).
