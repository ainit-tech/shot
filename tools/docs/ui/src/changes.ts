// What changed on a page since the previous version. The engine says which sections changed;
// here the page's own HTML is compared with the previous version's, block by block, and inside
// a changed paragraph, list item or table row, word by word.

import type { SectionChange } from './api';

/** Parts of the page that the site adds around the page's own text. */
const ADDED_BY_SITE =
  '.page-meta, .decision-eyebrow, .decision-banner, .decision-references, .page-history, .version-banner, .decision-list';

/** Longer blocks are shown as removed and added rather than compared word by word. */
const MAX_WORDS = 1500;

/** Blocks at least this alike are shown as one block changed; less alike, as removed and added. */
const MIN_LIKENESS = 0.5;

interface Block {
  el: Element;
  text: string;
}

interface Section {
  heading?: Element;
  blocks: Block[];
}

type Step<T> = { kind: 'same'; a: T; b: T } | { kind: 'del'; a: T } | { kind: 'ins'; b: T };

const normalize = (text: string | null) => (text ?? '').replace(/\s+/g, ' ').trim();

/** The text of a block; the cells of a table row are kept apart, so their words don't run together. */
const textOf = (el: Element) =>
  normalize(el.tagName === 'TR' ? Array.from(el.children, (cell) => cell.textContent).join(' ') : el.textContent);

/**
 * The blocks of a page by section: paragraphs, list items, table rows and the like, under the
 * id of their `h2` or `h3` heading; `''` is the part above the first such heading.
 */
function sectionsOf(root: Element): Map<string, Section> {
  const map = new Map<string, Section>([['', { blocks: [] }]]);
  let current = '';
  const visit = (parent: Element) => {
    for (const el of Array.from(parent.children)) {
      if (el.matches(ADDED_BY_SITE)) continue;
      if (el.tagName === 'H1') {
        current = '';
        map.get('')!.heading = el;
        continue;
      }
      if ((el.tagName === 'H2' || el.tagName === 'H3') && el.id) {
        current = el.id;
        map.set(current, { heading: el, blocks: [] });
        continue;
      }
      // Wrappers with headings inside, such as glossary cards, are read through.
      if (el.querySelector('h2[id], h3[id]')) {
        visit(el);
        continue;
      }
      const parts =
        el.tagName === 'UL' || el.tagName === 'OL'
          ? Array.from(el.children)
          : el.tagName === 'TABLE'
            ? Array.from(el.querySelectorAll('tr'))
            : [el];
      const section = map.get(current)!;
      for (const part of parts) section.blocks.push({ el: part, text: textOf(part) });
    }
  };
  visit(root);
  return map;
}

/** The longest common subsequence of two lists, as steps that turn `a` into `b`. */
function diff<T>(a: T[], b: T[], key: (item: T) => string): Step<T>[] {
  const ka = a.map(key);
  const kb = b.map(key);
  const n = a.length;
  const m = b.length;
  const lcs = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i][j] = ka[i] === kb[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const steps: Step<T>[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && ka[i] === kb[j]) steps.push({ kind: 'same', a: a[i++], b: b[j++] });
    else if (j >= m || (i < n && lcs[i + 1][j] >= lcs[i][j + 1])) steps.push({ kind: 'del', a: a[i++] });
    else steps.push({ kind: 'ins', b: b[j++] });
  }
  return steps;
}

/** How alike two texts are, from 0 to 1: the share of their words that they have in common. */
function likeness(a: string, b: string): number {
  const wa = a.split(' ').filter(Boolean);
  const wb = b.split(' ').filter(Boolean);
  if (wa.length + wb.length === 0) return 1;
  if (wa.length > MAX_WORDS || wb.length > MAX_WORDS) return 0;
  const common = diff(wa, wb, (w) => w).filter((step) => step.kind === 'same').length;
  return (2 * common) / (wa.length + wb.length);
}

/** A stretch of text: unchanged, or changed, with what was removed from it and what was added. */
interface Run {
  same: boolean;
  del: string;
  ins: string;
}

/** The text of `after` with the words removed since `before` struck out and the new ones marked. */
function words(before: string, after: string): Node[] | null {
  const a = before.match(/\s+|\S+/g) ?? [];
  const b = after.match(/\s+|\S+/g) ?? [];
  if (a.length > MAX_WORDS || b.length > MAX_WORDS) return null;
  const runs: Run[] = [];
  for (const step of diff(a, b, (w) => w)) {
    const same = step.kind === 'same';
    let run = runs.at(-1);
    if (!run || run.same !== same) runs.push((run = { same, del: '', ins: '' }));
    if (step.kind !== 'ins') run.del += step.a;
    if (step.kind !== 'del') run.ins += step.b;
  }
  // A short unchanged stretch between two changes, such as a lone "and", joins them, so that a
  // rewritten phrase reads as one removed and one added piece rather than a mix of fragments.
  // diff-match-patch cleans up its diffs the same way.
  const size = (run: Run) => Math.max(run.del.trim().length, run.ins.trim().length);
  for (let i = 1; i < runs.length - 1; ) {
    const [prev, run, next] = [runs[i - 1], runs[i], runs[i + 1]];
    if (run.same && !prev.same && !next.same && run.ins.trim().length <= Math.min(size(prev), size(next))) {
      runs.splice(i - 1, 3, { same: false, del: prev.del + run.del + next.del, ins: prev.ins + run.ins + next.ins });
      i = Math.max(1, i - 2);
    } else {
      i++;
    }
  }
  const nodes: Node[] = [];
  for (const run of runs) {
    if (run.same) {
      nodes.push(document.createTextNode(run.ins));
      continue;
    }
    // A change shows what was removed, then what was added.
    if (run.del.trim()) nodes.push(mark('del', run.del));
    if (run.ins.trim()) nodes.push(mark('ins', run.ins));
    else if (run.ins) nodes.push(document.createTextNode(run.ins));
  }
  return nodes;
}

function mark(tag: 'del' | 'ins', text: string): HTMLElement {
  const el = document.createElement(tag);
  el.textContent = text;
  return el;
}

/** A removed block, shaped to stand where `next` stands: a list item needs a list around it. */
function ghost(block: Block, next: Element): Element {
  let el = document.importNode(block.el, true) as Element;
  el.classList.add('diff-removed');
  const fits = el.tagName === next.tagName || (el.tagName !== 'LI' && el.tagName !== 'TR' && next.tagName !== 'LI' && next.tagName !== 'TR');
  if (!fits && el.tagName === 'LI') {
    const list = document.createElement('ul');
    list.append(el);
    el = list;
  } else if (!fits && el.tagName === 'TR') {
    const table = document.createElement('table');
    table.append(el);
    el = table;
  }
  return el;
}

/** Marks a block that is new in this version, or shows the words changed in it. */
function changed(before: Block, after: Block): boolean {
  if (before.el.tagName === 'TR' && after.el.tagName === 'TR') {
    const old = Array.from(before.el.children);
    const cells = Array.from(after.el.children);
    if (old.length !== cells.length) return false;
    cells.forEach((cell, i) => {
      const was = normalize(old[i].textContent);
      const now = normalize(cell.textContent);
      const nodes = was === now ? null : words(was, now);
      if (nodes) cell.replaceChildren(...nodes);
    });
  } else {
    const nodes = words(before.text, after.text);
    if (!nodes) return false;
    after.el.replaceChildren(...nodes);
  }
  after.el.classList.add('diff-changed');
  return true;
}

/**
 * Pairs removed and added blocks of one stretch of changes that are alike enough to be the same
 * block changed, keeping their order and making the pairs as alike as possible.
 */
function pairs(removed: Block[], added: Block[]): [number, number][] {
  const n = removed.length;
  const m = added.length;
  const like = removed.map((r) => added.map((a) => (r.el.tagName === a.el.tagName ? likeness(r.text, a.text) : 0)));
  const best = Array.from({ length: n + 1 }, () => new Float64Array(m + 1));
  const pair = (i: number, j: number) => (like[i][j] >= MIN_LIKENESS ? like[i][j] + best[i + 1][j + 1] : -1);
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) best[i][j] = Math.max(best[i + 1][j], best[i][j + 1], pair(i, j));
  }
  const found: [number, number][] = [];
  for (let i = 0, j = 0; i < n && j < m; ) {
    if (best[i][j] === pair(i, j)) found.push([i++, j++]);
    else if (best[i][j] === best[i + 1][j]) i++;
    else j++;
  }
  return found;
}

function compare(before: Section, after: Section): void {
  const steps = diff(before.blocks, after.blocks, (block) => block.text);
  let removed: Block[] = [];
  let added: Block[] = [];
  // One stretch of changes between unchanged blocks: removed blocks are shown struck through
  // before the added ones, and a removed block alike enough to an added one is shown as changed.
  const settle = (next: Element | null) => {
    let r = 0;
    let a = 0;
    const show = (upTo: number, place: Element | null) => {
      let anchor = place ? null : (after.blocks.at(-1)?.el ?? after.heading ?? null);
      for (; r < upTo; r++) {
        if (place) place.before(ghost(removed[r], place));
        else if (anchor) {
          const el = ghost(removed[r], anchor);
          anchor.after(el);
          anchor = el;
        }
      }
    };
    for (const [ri, ai] of pairs(removed, added)) {
      show(ri, added[a].el);
      for (; a < ai; a++) added[a].el.classList.add('diff-added');
      if (changed(removed[ri], added[ai])) r = ri + 1;
      else {
        show(ri + 1, added[ai].el);
        added[ai].el.classList.add('diff-added');
      }
      a = ai + 1;
    }
    show(removed.length, added[a]?.el ?? next);
    for (; a < added.length; a++) added[a].el.classList.add('diff-added');
    removed = [];
    added = [];
  };
  for (const step of steps) {
    if (step.kind === 'del') removed.push(step.a);
    else if (step.kind === 'ins') added.push(step.b);
    else settle(step.b.el);
  }
  settle(null);
}

/**
 * Shows the changes in `root`, which holds the page's own HTML, against `previousHtml`, the same
 * page in the previous version. Only the sections the engine reported are compared.
 */
export function showChanges(root: Element, previousHtml: string, sections: SectionChange[]): void {
  const old = document.createElement('div');
  old.innerHTML = previousHtml;
  const before = sectionsOf(old);
  const after = sectionsOf(root);
  for (const { id, status } of sections) {
    const section = after.get(id);
    if (!section) continue;
    const previous = before.get(id);
    if (status === 'added' || !previous) {
      for (const block of section.blocks) block.el.classList.add('diff-added');
    } else {
      compare(previous, section);
    }
  }
}
