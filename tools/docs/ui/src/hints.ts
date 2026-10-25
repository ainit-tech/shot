// Hints over glossary terms and links to decisions. The docs engine wraps terms in page texts
// into `<span class="term" data-term="…">`, and the site marks links to decision records with
// `decision-link`. Hovering or focusing one shows a card: a term's definition, or what a decision
// says. Tapping a term on a touch screen shows its card too; tapping a link simply opens it.

import type { TermInfo } from './api';

/** What the card over a link to a decision shows. */
export interface DecisionHint {
  /** "Decision 7". */
  label: string;
  title: string;
  status: string;
  statusName: string;
  date?: string;
  summary?: string;
  /** A line under the summary, such as what replaced the decision. */
  note?: string;
  url: string;
}

export interface HintSource {
  /** The term in the language and version of the page shown, or null while the terms load. */
  term(id: string): TermInfo | null;
  /** Text of the link to the term's card in the glossary. */
  moreLabel(): string;
  /** The decision a link leads to. */
  decision(link: HTMLAnchorElement): DecisionHint | null;
  /** Text of the link that opens the decision. */
  decisionMoreLabel(): string;
}

export interface Hints {
  hide(): void;
}

const SHOW_DELAY = 120;
const HIDE_DELAY = 150;
const GAP = 6;
const MARGIN = 8;
const ANCHORS = '.term, a.decision-link';

export function setupHints(root: HTMLElement, source: HintSource): Hints {
  const tip = document.createElement('div');
  tip.className = 'term-tip';
  tip.id = 'term-tip';
  tip.setAttribute('role', 'tooltip');
  tip.hidden = true;
  document.body.append(tip);

  let anchor: HTMLElement | null = null;
  let showTimer = 0;
  let hideTimer = 0;

  const anchorOf = (target: EventTarget | null) =>
    target instanceof Element ? target.closest<HTMLElement>(ANCHORS) : null;
  const isTerm = (element: HTMLElement) => element.classList.contains('term');

  function show(target: HTMLElement): void {
    window.clearTimeout(hideTimer);
    if (anchor === target && !tip.hidden) return;
    if (isTerm(target)) {
      const info = source.term(target.dataset.term ?? '');
      if (!info) return;
      fillTerm(info);
    } else {
      const info = source.decision(target as HTMLAnchorElement);
      if (!info) return;
      fillDecision(info);
    }
    anchor?.removeAttribute('aria-describedby');
    anchor = target;
    tip.hidden = false;
    target.setAttribute('aria-describedby', tip.id);
    place(target);
  }

  function fillTerm(info: TermInfo): void {
    const head = element('div', 'term-tip-head');
    head.append(element('strong', '', info.name), element('span', `status status-${info.status}`, info.statusName));

    const meta = element('div', 'term-tip-meta', `${info.kind} · `);
    info.code.forEach((code, i) => {
      if (i > 0) meta.append(', ');
      meta.append(element('code', '', code));
    });

    const text = element('div', 'term-tip-text');
    text.innerHTML = info.html;

    const more = element('a', 'term-tip-more', source.moreLabel());
    more.setAttribute('href', info.url);
    tip.replaceChildren(head, meta, text, more);
  }

  function fillDecision(info: DecisionHint): void {
    const head = element('div', 'term-tip-head');
    head.append(element('strong', '', info.label), element('span', `status status-${info.status}`, info.statusName));
    if (info.date) head.append(element('span', 'term-tip-date', info.date));

    const parts: HTMLElement[] = [head, element('div', 'term-tip-title', info.title)];
    if (info.summary) {
      const text = element('div', 'term-tip-text');
      text.append(element('p', '', info.summary));
      parts.push(text);
    }
    if (info.note) parts.push(element('div', 'term-tip-meta', info.note));

    const more = element('a', 'term-tip-more', source.decisionMoreLabel());
    more.setAttribute('href', info.url);
    tip.replaceChildren(...parts, more);
  }

  /** Below the anchor, or above it when there is no room below; always inside the window. */
  function place(target: HTMLElement): void {
    const box = target.getBoundingClientRect();
    tip.style.left = '0px';
    tip.style.top = '0px';
    const width = tip.offsetWidth;
    const height = tip.offsetHeight;
    const viewport = document.documentElement.clientWidth;
    const left = Math.max(MARGIN, Math.min(box.left, viewport - width - MARGIN));
    const below = box.bottom + GAP + height <= window.innerHeight || box.top - GAP - height < 0;
    const top = below ? box.bottom + GAP : box.top - GAP - height;
    tip.style.left = `${left + window.scrollX}px`;
    tip.style.top = `${top + window.scrollY}px`;
  }

  function hide(): void {
    window.clearTimeout(showTimer);
    window.clearTimeout(hideTimer);
    tip.hidden = true;
    anchor?.removeAttribute('aria-describedby');
    anchor = null;
  }

  function hideSoon(): void {
    window.clearTimeout(hideTimer);
    hideTimer = window.setTimeout(hide, HIDE_DELAY);
  }

  root.addEventListener('mouseover', (event) => {
    const target = anchorOf(event.target);
    if (!target) return;
    window.clearTimeout(showTimer);
    window.clearTimeout(hideTimer);
    showTimer = window.setTimeout(() => show(target), SHOW_DELAY);
  });
  root.addEventListener('mouseout', (event) => {
    const target = anchorOf(event.target);
    if (!target || target.contains(event.relatedTarget as Node | null)) return;
    window.clearTimeout(showTimer);
    hideSoon();
  });
  // Tapping a term on a touch screen shows its hint; tapping it again hides it.
  root.addEventListener('click', (event) => {
    const target = anchorOf(event.target);
    if (!target || !isTerm(target)) return;
    if (anchor === target && !tip.hidden) hide();
    else show(target);
  });
  root.addEventListener('focusin', (event) => {
    const target = anchorOf(event.target);
    if (target) show(target);
  });
  root.addEventListener('focusout', (event) => {
    if (anchorOf(event.target)) hideSoon();
  });

  tip.addEventListener('mouseenter', () => window.clearTimeout(hideTimer));
  tip.addEventListener('mouseleave', hideSoon);
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') hide();
  });
  document.addEventListener('click', (event) => {
    if (!anchorOf(event.target) && !tip.contains(event.target as Node)) hide();
  });
  window.addEventListener('scroll', hide, { passive: true });
  window.addEventListener('resize', hide);

  return { hide };
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className = '', text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}
