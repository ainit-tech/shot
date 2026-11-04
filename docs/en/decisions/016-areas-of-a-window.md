---
status: draft
date: 2026-11-03
topic: product
---

# Areas of a window

Switched off on November 4, 2026: the frozen screen lights up the window under the mouse again, and a click takes it. The parts found by the pixels of a window, as this page tells, didn't satisfy the author; a neural network of our own, trained on windows, is what to try next. The code stays in the program, switched off, with its tests.

Tried: while nothing is selected, the frozen screen lights up the area of the window under the mouse, a panel, a table, a message, a card, a picture or a paragraph, and a click without dragging selects it; with Ctrl held, the whole window lights up instead. The program finds the areas in the pixels of the frozen screen itself, by filling the surface under the mouse.

## Question

The frozen screen lit up the window under the mouse, and a click took it whole; a part of a window had to be dragged over. Yet a screenshot is more often of a part of a window than of all of it: a picture to ask Google about, a message to translate, an error, a chart, a paragraph. Should the frozen screen light up windows or their parts, which of them by default, and how is a part found?

## What is known (November 2026)

- On 11 screenshots of different programs, Chrome with a page of Wikipedia, LibreOffice Writer, Calc and Impress, GIMP, Thunderbird's calendar, Visual Studio Code, Telegram, Task Manager, qBittorrent and Explorer, filling found what a person sees: panels and docks whole, tables with their headings, whatever the colours of their cells, the week of a calendar, a list with a selected row in it, a slide of a gradient, the messages of a chat one by one and its wallpaper as one surface. Tabs and the bands of toolbars either light up the window or are small areas of their own. Checked on November 3, 2026.
- On 38 more screenshots, at the scales of 100% and 200%, the program found areas without failing. Checked on November 3, 2026.
- A whole window, looked at from a point every 24 pixels, takes 0.2 to 1.5 seconds on an 8-thread processor, and up to 5 on a screenshot compressed as JPEG. The slowest point, where a large surface is filled for the first time, takes 0.1 to 0.4 seconds; what has been filled lights up at once. Checked on November 3, 2026.
- On a wallpaper its large drawings light up as pictures of their own. Beside a page of a document the empty margins light up as rooms. Checked on November 4, 2026.
- On a page of BBC News the columns are 11 to 14 pixels apart, no further than the things inside an article, and chance empty rows across all three columns are 8 to 13 pixels high: neither distance nor empty bands tell the columns. What begins at the same place does: it gave the three columns, the articles between their lines and the seven cards with the text under their pictures. Checked on November 4, 2026.
- On a screenshot scaled and compressed as JPEG, noise joins the letters of neighbouring rows, and a table lights up by its columns. The program takes the screen itself, without any compression. Checked on November 3, 2026.
- Small neural networks don't find what is wanted here: ScreenParser (102 MB, 3.9 seconds a screenshot on a processor) finds buttons and lines of text but not panels, MobileSAM (1.7 to 3.9 seconds) finds shapes of no one size from one point to the next, and PP-DocLayout-S is for documents. Checked on November 3, 2026.

## Options

Which lights up by default:

| Option | For | Against |
|---|---|---|
| Parts, the window with Ctrl | The most frequent screenshots take a click | A part found wrong takes a drag |
| Windows, parts with a key held | A click does what it always did | The most frequent screenshots take a drag |

How a part is found:

| Option | For | Against |
|---|---|---|
| Filling the surface under the mouse: its walls, its furniture, its rooms and the doors between them | The same in every program, in pictures and remote screens too; finds what the frozen screen shows; a table, a list and a panel come out whole | Pieces of a picture light up inside it; a large surface takes a moment the first time |
| Cutting the window along its edges and its empty bands | Very quick | A band over panels of one colour, as an address bar over a navigation pane and a list, and tinted columns of a table cut it wrong |
| Small neural networks | Learned from real windows | Find elements, not panels; 100 MB and seconds on a processor |
| The accessibility of Windows (UI Automation) | The programs' own elements | Windows only, while the question is one of any picture; many programs tell it little or nothing, games and remote screens to begin with; what it tells may not match the frozen picture |

## Why

A question to Google, a translation and a screenshot to send are most often about a part of a window, and the window stays one key away. The pixels are what the user sees on the frozen screen, in every program. Cutting along edges was the first version, and it failed on what a person sees at once: a filled surface does show where it narrows into a new space, so a boundary is found where it is, not only where a straight line runs across the whole window.

## How it works

- The surface under the mouse is filled: its neighbouring pixels of one colour, taken from the smooth pixels nearest the mouse, and not from the inside of a letter.
- Thin long bars on the surface, alike all along, lines and scroll bars, are its walls. What the surface and its walls wholly surround, text, icons, pictures, cards, is its furniture. A lit row across a list, a selected item, is furniture too: the surface on either side of it is one surface.
- Where the surface narrows to a passage, it is two rooms with a door between them. They part if the passage is narrower than the narrower room's widest place by a quarter of it, by 8 pixels at the scale of 100% at least: a narrow panel beside a wide list is a room of its own, and a line under a tab that dents a long room doesn't part it.
- What stands in two rooms at once, as an address field over a navigation pane and a list, is a part of neither: they begin under it, and it makes a band of its own. The wide top or bottom of a room shaped as a T, as a menu bar over panels, is cut off from it, and the tops of rooms side by side at the same heights make one band.
- A surface that goes on smoothly past its colour, a gradient, a wallpaper of little drawings, is filled on the picture smoothed of them: much of its border is smooth, and the smoothed picture fills on from it to twice its size at least. A surface that ends at edges all round, a panel, a message, a field, is taken as it is, so the wallpaper of a chat is one surface and each message on it another.
- A table is found by the rhythm of its rows: rows of text of three cells or more, at one step, four rows at least, in columns that begin or end at the same places, with the rows of its heading over them. A row ends where another surface comes between its cells. The table lights up whole, whatever the colours of its cells, and is one piece of furniture of its room; across most of its rows' background, it is as wide as that background.
- The cells of a grid, a week of a calendar or a sheet, are surfaces of one colour parted by thin lines, as high as those beside them and as wide as those over and under them, in three rows and two columns at least. Any of them lights up the grid.
- A picture lights up whole: a busy part of the window, 64 by 56 pixels at the scale of 100% at least, a quarter of its box busy, no more than half of the window. The surface doesn't go in at a dark side of it.
- What stands on a surface is read as a page is. Things one under another that begin at the same place are a stack, a column or a card: so text belongs to the picture over it, whatever the distance to the next column. The thin lines across a stack part it into articles, and so does each picture of a stack of several. Stacks side by side no more than 40 pixels apart are a group, and what stands above all of them is their heading.
- What lights up depends on where the mouse is, and on nothing else: on a picture, the picture; on the text of an article or a card, all of it, its picture too; between two of them, within 24 pixels of both, what they are both in, the column or the group; in the empty space of a group, the group. Plain lines of text one under another are no article: they go by the rule below.
- Elsewhere the area under the mouse is the block of furniture under it, letters joined into words and lines into paragraphs, if it is 64 by 56 pixels at the scale of 100% at least and no more than 80% of the room; or else the room, 48 by 40 at least, or 26 high on a busy surface, a message of one line. Text gives way to the room when it is half of the room or more, or when the room is no more than 40% of the window: a card, a sidebar; in a large room, an article, the paragraph lights up. Around a room too small, a button or a field, the room around it is taken. A thin line along a side of the area, no longer than the side, is the area's own border and is taken in.
- On the window's frame the window lights up: on a band under 160 pixels high at the scale of 100% across 60% of the window at least, in the top quarter of the window or its bottom 15%, a title bar, toolbars, an address bar, a status bar; and so where the area would be 90% of the window or more.
- With Ctrl held, the whole window lights up, and pressing Ctrl or letting it go switches at once. Dragging selects any part, as it always did.

## Consequences

- A screenshot of a part takes a click where it took a drag.
- A part found wrong is still dragged over by hand, and Ctrl gives the window, as a click used to.
- The first look at a large surface can take a moment: nothing lights up anew during it.
- Nothing is asked of the user but where to point: a card, the gap between two cards, a margin.
- What lights up depends on how the window is drawn: in a window of very low contrast the window lights up more often than its parts.
