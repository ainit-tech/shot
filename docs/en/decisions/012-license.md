---
status: accepted
date: 2026-10-25
topic: product
---

# The program's license

Decided: ainitShot is free software under the GNU General Public License, version 3 or any later version (GPL-3.0-or-later). Anyone may use, study, change and pass on the program; whoever passes on a changed version passes on its source code under the same license.

## Question

ainitShot is published as open source ([decision](009-the-name-ainitshot.md)). Under which license, so that its code stays open and nobody makes a closed program of it?

## Options

| Option | For | Against |
|---|---|---|
| Closed, free of charge | The code stays with its author | Nobody can check what a program that sees the screen does with it; no free code signing for open-source projects |
| A permissive license: MIT, Apache-2.0 or BSD | The fewest conditions; usual for Rust | Anyone may close a copy, add advertising and sell it as their own |
| CC0-1.0, no rights kept | Nothing to follow | Not meant for programs, and not approved by the Open Source Initiative |
| GPL-3.0-only | Every copy stays open, under this very version | The code can't follow a later version of the GPL |
| GPL-3.0-or-later | Every copy stays open; the code can follow later versions of the GPL | The terms of later versions are set by the Free Software Foundation |

## Why

A program that sees the whole screen and sends pictures to Google should have code anyone can read. The GPL keeps every copy so: a changed version is passed on with its source code, so nobody can turn ainitShot into a closed program with advertising. The screenshot programs ShareX, Flameshot and Greenshot are under the GPL-3.0 too. "Or any later version" is the form the license's authors recommend.

## How it works

- The license's text is the file `LICENSE` at the root of the repository, and `Cargo.toml` names it: `GPL-3.0-or-later`.
- The libraries the program uses are under MIT, Apache-2.0, BSD, Zlib and similar licenses, which combine with the GPL-3.0. Their licenses ask for their notices to go with the program.
- The license covers the code; it gives no rights to the name ainitShot.

## Consequences

- A change someone else contributes comes under the same license.
- The Open Source Initiative approves the license, which free code signing for open-source projects requires.
