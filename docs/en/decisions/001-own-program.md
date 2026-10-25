---
status: accepted
date: 2026-10-21
topic: product
---

# Our own small program instead of Lightshot

We write our own small program with only the tools and actions listed in the [requirements](../requirements.md), instead of staying with Lightshot or moving to another ready-made program.

## Question

Lightshot has no filled rectangle, translation, queries with a mini prompt or magic pencil, and judging by its files, its Google search first puts the screenshot on the public prnt.sc. Is there a ready-made program that does what is needed, or is it simpler to write our own?

## Options (October 2026)

| Option | For | Against |
|---|---|---|
| Lightshot as it is | Already installed and familiar; 4 MB | Only a pen, line, arrow, rectangle, marker and text; no filled rectangle, translation or mini prompts; judging by its files, its Google search goes through a public upload to prnt.sc; closed and not developed: 5.5.0.7 is still the latest version |
| ShareX | Almost everything: rectangles with a fill, blur, step numbers, text recognition, Google Lens, an "Analyze image" button with one's own AI key | A big program (132 MB installer) with hundreds of settings; Google Lens only after uploading the screenshot to a host; no shape recognition |
| Flameshot | Small (19 MB), with a filled rectangle, blur and a counter | No search, no text recognition, no AI |
| Greenshot | Small (4 MB), with a filled rectangle, blur and text recognition | No image search and no translation |
| Snipping Tool, built into Windows | Shapes snap with "Draw & Hold"; text recognition; Bing Visual Search; Copilot | Only Bing and Copilot, no Google; can't be changed |
| Our own program | Exactly the tools and actions needed, the magic pencil and queries with mini prompts; nothing extra | Has to be written and maintained |

## Why

No ready-made program sends the screenshot with a question to Google's AI mode: the Snipping Tool sends it only to Copilot, and ShareX only to an AI service with one's own key. In the rest only the big ShareX comes close. The program needed is small: one window, seven tools, five actions. Writing it costs less than living with a big program cut down to a few features by its settings, and it can then grow exactly where needed.

## Consequences

- Good ideas are taken where they fit: the Snipping Tool snaps shapes the way the magic pencil is meant to ([decision](006-magic-pencil.md)).
- Lightshot stays until the program can replace it.
