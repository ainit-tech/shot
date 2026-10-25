# ainitShot documentation

- [English](en/README.md) — source of truth
- [Русский](ru/README.md) — translation
- [हिन्दी](hi/README.md) — translation

The documentation site and the data for AI agents are built from these files by [tools/docs](../tools/docs/README.md). From the repository root, `docker compose up -d` starts the site at http://shot-docs.ainit.localhost.

## Translation rules

1. Every change goes to the English version first, then to Russian and Hindi in the same change.
2. All three versions have the same folders and file names.
3. A translated page records in its front matter what it was translated from and when it was last updated: `translation_of: en` and `updated: YYYY-MM-DD`. Its `status` must match the original's.
4. Terms come only from the glossary. The [term base](en/glossary.md#term-base) lists every term in all three languages. Terms are edited in `glossary.toml`: the glossary's term cards, the term base and the hints over terms on the site are built from it.
5. Every diagram exists in all three languages with the same layout, and all three change together.

`docs check` verifies what it can of these rules: missing translations, front matter, links and the menu.

## Decision records

Why the project is designed the way it is lives in decision records, one file per decision: `<lang>/decisions/NNN-name.md`, three digits and then lowercase words. The pages that describe the project state only the result and link to the record: "why" for an accepted decision, "proposal" for a draft.

1. The front matter has `status` (`draft`, `accepted` or `superseded`), `date`, when the decision was made or proposed, and `topic`, one of the `decision_topics` in `site.toml`; a new topic is added there with a title in every language. A decision that replaces others lists their numbers in `supersedes`.
2. The paragraph right under the title states the decision: the site shows it in the list of decisions and in the hint over a link to the record. Then come the question, the options with what speaks for and against each, why, and the consequences.
3. A record states the decision as it stands. When it turns out wrong, incomplete or inaccurately recorded, the record itself is corrected, and the commit says what changed and why; the site keeps every earlier version and shows what changed. A record gets the status `superseded` only when another record now answers its question and lists it in `supersedes`.

`docs check` verifies the names, numbers, statuses, dates, topics and replacements.

### When a decision is accepted

1. **The record.** `status: accepted` and the `date` of the decision; the paragraph under the title states the decision without "Proposed:"; the sections are "Why", "How it works" and "Consequences"; what is still open moves to the open questions. Only what was actually agreed is recorded: when a proposal has several parts, each of them is confirmed.
2. **Terms.** New terms go to `glossary.toml` in every language, with their forms and the phrases in which a form means something else (`avoid`).
3. **Pages.** The pages that describe the project state the result and link to the record with "why".
4. **Open questions.** Questions the decision answers leave the list, and their section points to the record.
5. **Contradictions.** Other records and pages are searched for what the decision changes: words, numbers, examples. What contradicts it is corrected in place, in the same change.
6. **Translations** change together with the original, with `updated`.
7. **Renaming.** When a record's file is renamed together with a large rewrite, the rename goes into a commit of its own first, so that the page's history follows the file.
8. `docs check` passes, and the decision is committed on its own.
