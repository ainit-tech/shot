---
status: accepted
date: 2026-10-21
topic: drawing
---

# Magic pencil: a neat shape from a rough loop

When the mouse button is released, the magic pencil replaces a rough loop with the neat shape that fits it best, an ellipse or a rectangle. The shape is computed from the points of the loop alone, without AI. An outline along the edge of the object, found by a segmentation model on the computer, starts from the same loop and can become a second mode of the tool.

## Question

What should a loop drawn roughly around something turn into, and does that need AI?

## What is known (October 2026)

- Turning ink into neat shapes is common: Ink to Shape in OneNote and the other Microsoft 365 apps, shape snapping in iPadOS Notes, and "Draw & Hold" in the Windows Snipping Tool, which uses the shape recognizer built into Windows (InkAnalyzer).
- Fitting an ellipse to points is a small, well-known calculation: the direct least-squares method of Fitzgibbon, Pilu and Fisher, `fitEllipseDirect` in OpenCV. The corners of a rectangle are found by simplifying the stroke.
- Segmentation models find the outline of an object from a rough hint: a box and points inside and outside it. The small ones run on the processor. MobileSAM (Apache 2.0) takes about 45 MB as ONNX and, in a benchmark on a server processor, about 0.4 s per picture, of which reading the hint takes about 50 ms. EfficientViT-SAM-L0 is faster, about 0.2 s, but larger, about 140 MB. The Snipping Tool itself ships a segmentation model of this size. From Rust such models run through the `ort` crate (ONNX Runtime).

## Options

| Option | For | Against |
|---|---|---|
| A neat shape fitted to the loop: an ellipse, or a rectangle when the loop has four corners | Instant; works offline; always predictable; about a hundred lines of code | Doesn't look at the picture: the shape doesn't follow the object |
| The shape recognizer built into Windows (InkAnalyzer), as in the Snipping Tool | Ready-made; knows many shapes | Not checked whether it works in a program without an app package; ties the tool to the Windows ink API |
| An outline along the edge of the object inside the loop, found by a segmentation model on the computer (MobileSAM through ONNX Runtime) | Looks like magic: the outline hugs the object | Adds about 60 MB with the model and the runtime; sometimes picks the wrong object; much more code |
| The same with a cloud AI | No model on the computer | The screenshot leaves the computer for every loop; a delay; a key and the internet |

## Why

A loop on a screenshot says "look here". A neat ellipse or rectangle says it just as well, at once, without a model and without covering the object's own edge. An outline along the edge pays off only where neighbouring things make a neat shape ambiguous. Both start from the same loop, so the outline can be added as a second mode without changing the first.

## How it works

- While the loop is being drawn, it follows the mouse like the pencil.
- When the button is released and the ends of the loop are close, both an ellipse and a rectangle are fitted to it, and the one with the smaller error wins. If neither fits well, the stroke stays as drawn.
  - A loop drawn on past its start ends where it reached the start: the part drawn twice would otherwise weigh twice and pull the ellipse off the loop.
  - The ellipse keeps the loop's proportions however long it is: its radii are fitted by least squares, since the points of a long loop crowd on its long sides.
  - Tried on loops drawn as by hand, careful, usual and hasty, circles and ovals up to three times as long as high became ellipses, and boxes rectangles; a shape between the two, a box with very round corners, becomes an ellipse.
- Holding Shift on release keeps the stroke as drawn.
- If the outline along the edge is added, the model reads the selection in the background as soon as it is made, so the outline appears about 50 ms after the button is released.

## Consequences

- The magic pencil needs neither a model nor the network.
