---
status: accepted
date: 2026-11-03
topic: product
---

# Areas of a window

Decided: while nothing is selected, the frozen screen lights up the area of the window under the mouse, a picture, a message, a card, a panel or a paragraph, and a click without dragging selects it; with Ctrl held, the whole window lights up instead. The program finds the areas in the pixels of the frozen screen itself.

## Question

The frozen screen lit up the window under the mouse, and a click took it whole; a part of a window had to be dragged over. Yet a screenshot is more often of a part of a window than of all of it: a picture to ask Google about, a message to translate, an error, a chart, a paragraph. Should the frozen screen light up windows or their parts, which of them by default, and how is a part found?

## What is known (November 2026)

- On 30 screenshots of popular programs from Wikimedia Commons, at the scales of 100% and 200%, of Chrome, Edge and Firefox with pages of Wikipedia, LibreOffice Writer, Thunderbird's mail and calendar, Visual Studio Code, Notepad++, windows of Windows and Steam, the parts found by their pixels were those a person sees: pictures to their edges, boxes and cards with their borders, sidebars, groups of links, paragraphs, the editor, the terminal. Tabs, toolbars, a ribbon and a status bar came out as the window's frame, not as parts. Checked on November 3, 2026.
- Lighting up a part takes no time that shows: all the parts of a whole screenshot, looked for from a point every 24 pixels, were found in 0.1 to 0.5 seconds. Checked on November 3, 2026.
- A page whose sidebar's grey goes on above the article, as Wikipedia's did in 2018, has no edge between the two along its whole height, and its sidebar lights up as groups of links rather than whole. Checked on November 3, 2026.

## Options

Which lights up by default:

| Option | For | Against |
|---|---|---|
| Parts, the window with Ctrl | The most frequent screenshots take a click | A part found wrong takes a drag |
| Windows, parts with a key held | A click does what it always did | The most frequent screenshots take a drag |

How a part is found:

| Option | For | Against |
|---|---|---|
| The pixels of the frozen screen: its edges and its empty bands | The same in every program, in pictures and remote screens too; finds what the frozen screen shows; quick | Boundaries of very low contrast are missed, and so are layouts not built of straight boxes |
| The accessibility of Windows (UI Automation) | The programs' own elements | Many programs tell it little or nothing, games and remote screens to begin with; slow across programs; what it tells may not match the frozen picture |

## Why

A question to Google, a translation and a screenshot to send are most often about a part of a window, and the window stays one key away. The pixels are what the user sees on the frozen screen, in every program, and they are quick to read.

## How it works

- A window is built of boxes: panels with surfaces of their own and lines between them, and on them blocks of content with empty space around them. The window is cut along its edges, where one smooth surface meets another all across it, or else along its gaps, the empty bands across it, the widest first; every part is cut again the same way.
- A part with a surface of its own is a panel, its border with it; so is a box drawn with lines. Parts of one size in a row, the cells of a grid, of a table or of a list, are not parts of their own, and neither is empty space.
- The area under the mouse is the smallest panel or block around it that is big enough: at the scale of 100%, a panel of 48 by 40 pixels, a block of content of 64 by 56, more than two lines of text. Nothing inside a thin strip is an area, such as a tab or a line of text. Text in a small panel, a card, a message, a sidebar, lights up the panel; in a large one, an article, the paragraph.
- On the window's frame, the chain of bands across the window from its top or its bottom, and down it from its sides, the window lights up: on its title bar, tabs, toolbars, a ribbon, a ruler, a status bar.
- With Ctrl held, the whole window lights up, and pressing Ctrl or letting it go switches at once. Dragging selects any part, as it always did.

## Consequences

- A screenshot of a part takes a click where it took a drag.
- A part found wrong is still dragged over by hand, and Ctrl gives the window, as a click used to.
- What lights up depends on how the window is drawn: in a window of very low contrast, or not built of straight boxes, the window lights up more often than its parts.
