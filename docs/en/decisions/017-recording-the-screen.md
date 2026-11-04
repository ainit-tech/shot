---
status: draft
date: 2026-11-04
topic: product
---

# Recording the screen

Tried: the toolbar of the actions has one more button, Record, Ctrl+R. It records what is selected: the frozen screen goes, the screen goes on living inside a thin frame, and after a count of three a small panel in place of the toolbar shows the time, pause and stop. The hotkey stops the recording too. Then a window plays what was recorded, two handles keep a part of it, and a switch makes it a GIF or a video before Copy or Save.

## Question

A screenshot doesn't show what moves: how a bug happens, where to click, what a program does. A short GIF or video says that better, and sending one takes another program today. How can recording fit into a program made for a hotkey and a selection, without changing how that works?

## What is known (November 2026)

- Reading the screen through GDI, as the frozen screen does, makes the mouse flicker when done many times a second, and it is slow on large screens. The graphics card hands over the picture of a monitor by itself (Desktop Duplication) without either problem, but only of one monitor at a time.
- Windows has H.264 in Media Foundation, on the graphics card where it can, so nothing has to be built into the program. The N editions of Windows come without it. Programs that link to it don't start there, so the program looks for it when it is needed.
- At 1920 by 880 five seconds of a real page, moving and then still, went into an MP4 file of 1.2 MB in about 2 seconds; at 2736 by 1624 in about 4.5 seconds, 14 MB. Checked on November 4, 2026.
- A GIF of those five seconds, 1000 pixels wide and at 15 frames a second, was made in 2.5 seconds and came to 306 KB; of a chat on a patterned wallpaper, 6.7 MB against 3.3 MB of the video. So a GIF is good for short recordings of plain windows, and a video for everything else. Checked on November 4, 2026.
- Windows' reader converts a frame of 1920 by 1080 in 36 ms by default and in 9 ms with its advanced processing. Checked on November 4, 2026.
- The encoder of Windows writes the colours of small areas by the old standard for small pictures, whatever it is told. A player shows them a little off: a blue of (47, 111, 235) as (59, 114, 236). At HD sizes the colours are right.

## Options

Where recording starts:

| Option | For | Against |
|---|---|---|
| A button on the toolbar of the actions, after the selection | Nothing changes in how the program works; what to record is chosen the way it always is | Two steps before recording |
| A hotkey of its own | One step | One more key to remember; a selection is needed anyway |

When the format is chosen:

| Option | For | Against |
|---|---|---|
| After the recording | The right format can be seen by the length and the sizes; nothing has to be decided in advance | Making a GIF takes a moment |
| Before the recording | Nothing to make afterwards | The size and the length aren't known yet |

## Why

The program's way of working stays: the hotkey, the selection, an action that finishes the work. Recording is one more action, so there is nothing new to learn, and the area comes from the same selection, with the windows lit up. The format comes last because only then is it known what came out; recording goes into an MP4 file anyway, from which a GIF takes a moment and which can be cut.

## How it works

- **The button** stands on the toolbar of the actions, before masking; what is drawn on the picture stays behind with the frozen screen.
- **The count.** The frame is grey for three seconds, for the mouse to get where it should; Stop or the hotkey gives up during the count, and nothing is left behind. Then the frame goes red and the recording begins.
- **The frame and the panel** are not in the recording, even when the panel has to stand inside the area, on a whole screen: Windows leaves them out of what is read of the screen. Clicks go through the frame. The panel takes no focus away.
- **The recording** is an MP4 file of H.264 at 30 frames a second, or as many as the computer manages, with their real times. A pause is left out of the time. The mouse pointer is in the picture; sound is not. The width and the height are even, as H.264 wants; an area beyond 4096 by 2304 is made smaller.
- **The window of the result** plays the recording in a loop. The handles keep a part of it; a click on the line goes to a place in it. The file is made at once on a thread of its own and its size is shown; a new choice gives up what was being made.
- **GIF or video.** A recording of up to 15 seconds opens as a GIF, a longer one as a video. A GIF has 15 frames a second and at most 1000 pixels of width; each frame keeps only what has changed, with 255 colours of its own. A video that isn't cut is the file itself; one that is cut goes through the encoder again.
- **Copy** puts the file into the clipboard, as Explorer copies one, under the name of the pattern from the settings. **Save** puts it into the folder from the settings, or the Videos folder if none is set, and asks where if the settings say so. Then the window closes, as a screenshot does.
- **What is left** stays in the temporary folder for a day, for what has been copied to be pasted.

## Consequences

- A GIF takes as long to make as about half of what it shows at a size of a whole screen, and more on larger screens: the window says how far it has come.
- Sound, a mark around clicks and drawing during a recording are left for later.
- Whole-screen games in their own mode may not be recorded.
