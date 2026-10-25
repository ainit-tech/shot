# Documentation tools

Builds the documentation site from `docs/`, together with Markdown for AI agents and a JSON API for programs.

- `engine/` — `docs`, a Rust command-line tool. It checks the sources and writes the site data.
- `ui/` — the site itself: Vite and TypeScript without a framework. It reads the same JSON API that programs use. Text is set in Inter and code in JetBrains Mono; the fonts are served with the site from `ui/src/fonts` under the SIL Open Font License, so pages load nothing from other addresses. Hindi uses the system's Devanagari font. The top bar shows the site's address, screenshot.ainit.localhost, and every tab title starts with its first part, `screenshot`, so the site's tabs stand out among those of other local sites.

## Commands

From `tools/docs/ui` (run `npm install` once):

| Command | What it does |
|---|---|
| `npm run dev` | Builds the data and starts the site at http://localhost:5176. While it runs, every change in `docs/` (or in the engine's code) rebuilds the data, and open pages update in place, keeping the scroll position. If the checks fail, the errors appear in the browser until they are fixed |
| `npm run build` | Builds the data and the site into `ui/dist` |
| `npm run data` | Only builds the data: `cargo run -p docs-engine -- build` |

The engine alone, from anywhere in the repository:

```
cargo run -p docs-engine -- check
cargo run -p docs-engine -- build
```

`check` prints errors and warnings; errors make it exit with code 1, and `build` refuses to write anything while there are errors.

`build` replaces the files of the site data one by one, each written beside its place and then renamed into it, and removes the files it no longer writes only at the end. A site that is running while the data is rebuilt never finds it half-written or missing.

## Running in a container

The site can also run in Docker, on its own, apart from any terminal session. From the repository root:

| Command | What it does |
|---|---|
| `docker compose up -d` | Builds the image the first time and starts the site at http://screenshot.ainit.localhost (through the machine's shared local gateway; no port is opened on the host). The site keeps running, and Docker starts it again whenever Docker itself starts |
| `docker compose logs -f docs` | Shows the output: the rebuilds after every change and the errors of the checks |
| `docker compose down` | Stops the site and removes its container, so Docker no longer starts it |

The image holds only the tools: Rust, Node.js and git. The repository is mounted into the container, so the site shows the docs as they are on disk and rebuilds after every change, as `npm run dev` does. Changes made on a Windows or macOS host don't reach the container as file events, so there the files are checked once a second (`DOCS_POLL`). The container keeps its packages, build output and generated site data in Docker volumes, apart from the host's `node_modules`, `target` and `public`.

The first start takes a few minutes: the container installs the packages and builds the engine. The container opens no port on the host, so `npm run dev` can run beside it at http://localhost:5176.

## Versions and history

Versions come from Git:

- **latest** is the working tree, including changes that are not committed yet;
- every commit on the main line that changes `docs/` is a version. A milestone is a Git tag `vX.Y.0`, set by hand; every other commit takes the next patch number after the version before it and needs no tag, so after `v0.3.0` come `v0.3.1`, `v0.3.2` and so on.

A version is published at `/v0.3.2/<lang>/<page>`, and the site shows which version you are reading. The version menu groups versions by their first two numbers and marks milestones. Every page also carries its history: the commits that changed it, newest first, each with the version it made, and a mark if the page has uncommitted changes. Only author names are published, not e-mail addresses.

Each version is compared with the one before it. The latest version is compared with the newest commit when the docs have uncommitted changes, and otherwise with the version before that commit. On the site:

- the menu says how many pages changed, and marks every new or changed page;
- the table of contents marks the sections that are new or changed, and so does a mark after their headings;
- the line under a page's title says what it is compared with, and "Show the changes" shows them in the text: new blocks, removed ones struck through, and inside a changed paragraph, list item or table row, the words that changed. A block rewritten so much that less than half of its words remain is shown as removed and added instead, and a short unchanged piece between two changes joins them, so a rewritten phrase reads as one piece rather than a mix of fragments.

The marks of changes have a colour of their own, so they are never taken for a status: amber means a draft and green means accepted. Only in the changes shown in the text is added text green and removed text red, as in any diff.

The files of a version are written once: a later build keeps them while the commit and the engine stay the same, so a rebuild writes only the latest version. Problems in an older version are reported as warnings and do not stop the build: the past cannot be fixed.

## Three kinds of readers

| Reader | URLs | Contents |
|---|---|---|
| People | `/<lang>/<page>`, `/<version>/<lang>/<page>` | The site |
| AI agents | `/llms.txt`, `/llms-full.txt`, `/md/<version>/<lang>/…` | Markdown sources |
| Programs | `/api/…` | JSON |

- `llms.txt` ([llmstxt.org](https://llmstxt.org)) — project summary, the API entry points and links to the Markdown of every page in every language.
- `llms-full.txt` — all pages of the source language in one file.
- `/md/<version>/<lang>/…` — the source files with front matter; the glossary page has its generated parts filled in. Relative links and diagrams inside them work.

### JSON API

Every file has `schemaVersion`, raised when a field is removed or changes meaning.

| URL | Contents |
|---|---|
| `/api/versions.json` | Versions, newest first: `id`, `commit`, `date`, `tagged` for a milestone, `previous` — the version it is compared with — and `manifest` |
| `/api/<version>/manifest.json` | Name, summary, languages, menu, `decisionTopics`, `previous`, and every page's metadata with its `route`, `apiUrl`, `markdownUrl`, `lastChange`, and `changes` against the previous version: `status`, the new and changed `sections` by heading id, the titles of `removed` ones; a decision record also has `decision`: `number`, `date`, `topic`, `summary`, `supersedes`, `supersededBy` |
| `/api/<version>/pages/<lang>/<page>.json` | One page: metadata, `translations`, `headings`, `history`, `uncommittedChanges`, `changes`, `markdown` and `html`; a decision record also has `decision` and `referencedBy`, the pages that link to it |
| `/api/<version>/search/<lang>.json` | Search index |
| `/api/<version>/terms/<lang>.json` | Glossary terms by id: `name`, `code`, `kind`, `status`, `statusName`, the definition as `html`, and `url` of the term's card |

## Sources

- `docs/site.toml` — site name and summary, languages, the menu, and the topics of decision records (`decision_topics`: an `id` and a `title` per language, in display order). A menu section is a folder; its `README.md` is the section's own page, set as `index`: the section takes its title from that page and links to it, so the page has no separate menu entry. A per-language `title` is only for a section without such a page.
- `docs/glossary.toml` — the term base, see [Glossary and term hints](#glossary-and-term-hints).
- `docs/<lang>/**/*.md` — pages. A page is identified by its path without `.md`; a folder's `README.md` is identified by the folder path, and the language's own `README.md` is `index`.
- Any other file in a language folder (diagrams) is published next to the Markdown as is.

Each page starts with a `# Title` heading. Metadata goes in front matter at the top of the file: flat `key: value` lines between `---` lines.

| Key | Meaning |
|---|---|
| `status` | `draft`, `accepted`, `partial` or `living`; a decision record has `draft`, `accepted` or `superseded` |
| `note` | A sentence that clarifies the status |
| `nav_title` | A shorter title for the menu |
| `date` | Decision records only: when the decision was made or, for a draft, proposed, `YYYY-MM-DD` |
| `supersedes` | Decision records only: the numbers of the decisions this one replaces, `3` or `3, 5` |
| `topic` | Decision records only: the topic, one of `decision_topics` in `site.toml` |
| `translation_of` | Translations only: the language of the original, `en` |
| `updated` | Translations only: when the text was last updated from the original, `YYYY-MM-DD` |

Links between pages are ordinary relative links to `.md` files, so they also work on GitHub; the engine turns them into site links.

## Decision records

A decision record is `docs/<lang>/decisions/NNN-name.md`; its number comes from the file name, so `007-ansible.md` is decision 7. The paragraph right under its title states the decision and serves as its summary. The rules for writing records are in `docs/README.md`.

The decisions page, `decisions/README.md`, holds the marker `<!-- decisions: list -->`. In the Markdown sources the engine replaces it with a list of all records; on the site the list is drawn from the manifest. The menu shows the decisions section rather than every record, so records need no entries in `site.toml`.

On the site:

- the list of decisions is grouped by topic, in the order of `decision_topics`, has filters by status, and shows each decision's number, title, summary, status and date; versions written before topics show one list;
- the menu shows how many drafts are waiting for a decision, and inside the section it lists the decisions themselves, under their topics as on the decisions page;
- a draft is marked in amber, the colour of drafts, wherever it appears: by a dot before its title in the menu, by its status in the list, and on its number on links in the texts;
- a record shows its number and topic above the title, its status, date and replacements under it, a notice when a later decision replaced it, and at the end the pages that link to it and the neighbouring decisions by number, in the order they were made;
- a link to a record anywhere in the texts gets the decision's number, and hovering over it or focusing it shows the decision's status, date and summary;
- search results show the number of a decision.

## Glossary and term hints

Terms live in `docs/glossary.toml`, every term in every language: its name, kind, definition, notes, code names and status. From this file the engine:

- fills in the glossary page. `docs/<lang>/glossary.md` holds only the prose and two markers: `<!-- glossary: term base -->` for the table of all terms in all languages, and `<!-- glossary: terms -->` for the term cards grouped into sections;
- marks terms in the texts of every page for the hints on the site. A term is found by its `forms`, the word forms listed for each language; letter case does not matter, and the longest form wins, so "control server" is not taken for "server". `avoid` lists phrases in which a form means something else, such as "guest agent". Headings, links, tables and code are left alone, and so is a card's own term;
- writes `/api/<version>/terms/<lang>.json` for the hints.

On the site, the first mention of a term in each section is underlined with dots. Every mention shows a hint with the definition on hover, keyboard focus or tap, with a link to the term's card.

When a word is marked where it means something else, rephrase the text or add the phrase to `avoid`.

## Checks

- every file of the source language exists in every other language, and nothing more;
- front matter: known keys, valid status, translation fields, the same status as the original;
- every page has a `# Title`;
- every relative link leads to an existing page, heading or file and stays inside the language folders;
- every menu page exists in every language; pages missing from the menu are warnings;
- the term base: every term has a name, kind, definition and forms in every language, known sections and statuses, and no form belongs to two terms; every glossary page has both markers;
- decision records: named `NNN-name.md` with unique numbers, a status, a date and a known topic, a summary under the title, the same `date`, `topic` and `supersedes` in every language; every topic has a title in every language; a replaced decision is `superseded` by one that lists it in `supersedes`; the list marker is on the decisions page and nowhere else.

## Publishing

`ui/dist` after `npm run build` is a static site. The web server must:

- send `index.html` for paths that are not files (the site routes pages itself);
- send `.md` and `.txt` files with `charset=utf-8`, otherwise browsers garble Russian and Hindi. The Vite dev and preview servers already do both.

## Limits of this version

- The site needs JavaScript; readers without it are pointed to `llms.txt` and the API.
- Diagrams follow the system's light or dark theme, not the theme chosen on the site.
