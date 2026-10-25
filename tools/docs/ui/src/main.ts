import './style.css';
import {
  fetchJson,
  LATEST,
  type DecisionInfo,
  type DecisionTopic,
  type Manifest,
  type PageChanges,
  type PageData,
  type PageEntry,
  type TermsFile,
  type VersionInfo,
  type Versions,
} from './api';
import { showChanges } from './changes';
import { setupHints, type DecisionHint } from './hints';
import { strings, type DecisionFilter, type Strings, type Theme } from './i18n';
import { Search } from './search';

/** A page of a documentation version. */
interface Place {
  version: string;
  lang: string;
  id: string;
}

/** The folder of decision records; its own page lists them. */
const DECISIONS = 'decisions';
const DECISION_FILTERS: DecisionFilter[] = ['all', 'draft', 'accepted', 'superseded'];

/** A decision record in the manifest shown. */
interface Decision {
  id: string;
  entry: PageEntry;
  info: DecisionInfo;
}

let versions: Versions;
let latest: Manifest;
try {
  versions = await fetchJson<Versions>('/api/versions.json');
  latest = await fetchJson<Manifest>(`/api/${LATEST}/manifest.json`);
} catch (error) {
  document.body.textContent = `Could not load the site data from /api/. Build it with "npm run data". ${error}`;
  throw error;
}

const $ = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
const ui = {
  topbar: $('#topbar'),
  brand: $('.brand'),
  menu: $<HTMLButtonElement>('#menu-toggle'),
  search: $<HTMLInputElement>('#search'),
  results: $<HTMLUListElement>('#search-results'),
  version: $<HTMLSelectElement>('#version'),
  languages: $('#languages'),
  theme: $<HTMLButtonElement>('#theme'),
  sidebar: $('#sidebar'),
  content: $('#content'),
  toc: $('#toc'),
};

/** Tab titles start with the first part of the site's address in the top bar, `shot-docs` for
 * shot-docs.ainit.localhost, so the site's tabs stand out among those of other local sites. */
const tabName = (ui.brand.textContent ?? '').trim().split('.')[0];

const manifests = new Map<string, Manifest>([[LATEST, latest]]);
const pages = new Map<string, PageData>();
const searches = new Map<string, Search>();
/** Glossary terms by `version/lang`; null for a version without a glossary. */
const termFiles = new Map<string, TermsFile | null>();
let current: Place = { version: LATEST, lang: latest.defaultLanguage, id: 'index' };
let manifest = latest;
let shownPath = '';
let renderToken = 0;
/** The page shows its changes against the previous version; see changes.ts. */
let changesShown = false;

const storage = {
  get(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key: string, value: string): void {
    try {
      localStorage.setItem(key, value);
    } catch {
      // Storage can be unavailable (private mode); the choice then lasts for this visit only.
    }
  },
};

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  text?: string,
  className?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (className) node.className = className;
  return node;
}

function link(label: string, href: string, newTab = false): HTMLAnchorElement {
  const a = element('a', label);
  a.href = href;
  if (newTab) {
    a.target = '_blank';
    a.rel = 'noopener';
  }
  return a;
}

const text = (): Strings => strings[current.lang] ?? strings.en;

const hints = setupHints(ui.content, {
  term: (id) => termFiles.get(`${current.version}/${current.lang}`)?.terms[id] ?? null,
  moreLabel: () => text().termMore,
  decision: (a) => decisionHint(Number(a.dataset.decision)),
  decisionMoreLabel: () => text().openDecision,
});

const icon = (paths: string) =>
  `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;

const ICONS: Record<Theme | 'menu', string> = {
  auto: icon('<circle cx="8" cy="8" r="6"/><path d="M8 2a6 6 0 0 1 0 12z" fill="currentColor"/>'),
  light: icon(
    '<circle cx="8" cy="8" r="2.8"/><path d="M8 1.5v1.6M8 12.9v1.6M1.5 8h1.6M12.9 8h1.6M3.4 3.4l1.1 1.1M11.5 11.5l1.1 1.1M3.4 12.6l1.1-1.1M11.5 4.5l1.1-1.1"/>',
  ),
  dark: icon('<path d="M13.5 10A6 6 0 0 1 6 2.5a6 6 0 1 0 7.5 7.5z"/>'),
  menu: icon('<path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h11"/>'),
};

/** Site path of a page: `/en/glossary` for the latest version, `/v0.1.0/en/glossary` for a tagged one. */
function route({ version, lang, id }: Place): string {
  const prefix = version === LATEST ? '' : `/${version}`;
  return id === 'index' ? `${prefix}/${lang}/` : `${prefix}/${lang}/${id}`;
}

function parse(pathname: string): Place | null {
  const parts = decodeURIComponent(pathname).split('/').filter(Boolean);
  let version = LATEST;
  if (parts.length > 0 && parts[0] !== LATEST && versions.versions.some((v) => v.id === parts[0])) {
    version = parts.shift()!;
  }
  const [lang, ...rest] = parts;
  return lang ? { version, lang, id: rest.join('/') || 'index' } : null;
}

function preferredLanguage(): string {
  const saved = storage.get('lang');
  if (saved && latest.pages[saved]) return saved;
  for (const tag of navigator.languages) {
    const code = tag.split('-')[0];
    if (latest.pages[code]) return code;
  }
  return latest.defaultLanguage;
}

async function loadManifest(version: string): Promise<Manifest | null> {
  const cached = manifests.get(version);
  if (cached) return cached;
  const info = versions.versions.find((v) => v.id === version);
  if (!info) return null;
  const loaded = await fetchJson<Manifest>(info.manifest);
  manifests.set(version, loaded);
  return loaded;
}

async function loadPage(url: string): Promise<PageData> {
  let page = pages.get(url);
  if (!page) {
    page = await fetchJson<PageData>(url);
    pages.set(url, page);
  }
  return page;
}

async function loadTerms(version: string, lang: string): Promise<void> {
  const key = `${version}/${lang}`;
  if (termFiles.has(key)) return;
  try {
    termFiles.set(key, await fetchJson<TermsFile>(`/api/${version}/terms/${lang}.json`));
  } catch {
    // Versions made before the glossary became data have no terms: pages simply show no hints.
    termFiles.set(key, null);
  }
}

function navigate(url: string): void {
  const target = new URL(url, location.href);
  if (target.pathname !== shownPath) changesShown = false;
  ui.search.value = '';
  history.pushState(null, '', target.pathname + target.hash);
  void show(target.pathname, target.hash);
}

async function show(pathname: string, hash: string, keepScroll = false): Promise<void> {
  const token = ++renderToken;
  const place = parse(pathname);
  const found = place ? await loadManifest(place.version) : null;
  if (token !== renderToken) return;
  if (!place || !found?.pages[place.lang]) {
    const home = route({ version: LATEST, lang: preferredLanguage(), id: 'index' });
    history.replaceState(null, '', home);
    return show(home, '');
  }
  current = place;
  manifest = found;
  shownPath = pathname;
  const t = text();
  document.documentElement.lang = place.lang;
  document.body.classList.remove('menu-open');
  hideResults();
  hints.hide();
  renderChrome(t);
  renderVersions(t);
  renderLanguages();
  renderSidebar(t);

  const entry = manifest.pages[place.lang][place.id];
  if (!entry) {
    ui.content.replaceChildren(element('h1', t.notFound), element('p', t.notFoundHint));
    ui.toc.replaceChildren();
    document.title = `${tabName} · ${t.notFound}`;
    return;
  }
  const [page] = await Promise.all([loadPage(entry.apiUrl), loadTerms(place.version, place.lang)]);
  if (token !== renderToken) return;
  ui.content.innerHTML = page.html;
  if (changesShown && page.changes?.status === 'changed' && manifest.previous) {
    try {
      const before = await loadPage(`/api/${manifest.previous}/pages/${place.lang}/${place.id}.json`);
      if (token !== renderToken) return;
      showChanges(ui.content, before.html, page.changes.sections ?? []);
    } catch {
      // The previous version has no such page: there is nothing to compare with.
    }
  }
  const title = ui.content.querySelector('h1');
  const meta = pageMeta(page, t);
  if (title) title.after(meta);
  else ui.content.prepend(meta);
  if (page.decision) {
    (title ?? meta).before(decisionEyebrow(page.decision, t));
    if (page.decision.supersededBy?.length) meta.after(supersededBanner(page.decision.supersededBy, t));
    ui.content.append(decisionFooter(page, t));
  }
  for (const holder of ui.content.querySelectorAll<HTMLElement>('.decision-list')) renderDecisionList(holder, t);
  markDecisionLinks();
  if (place.version !== LATEST) ui.content.prepend(versionBanner(t));
  if (page.changes) markChangedHeadings(page.changes, t);
  ui.content.append(pageHistory(page, t));
  renderToc(page, t);
  document.title = page.decision
    ? `${tabName} · ${t.decision(page.decision.number)}. ${page.title}`
    : `${tabName} · ${page.title}`;
  if (!keepScroll) scrollToHash(hash);
}

/** Loads the data again after the docs were rebuilt and redraws the page in place. */
async function reloadData(): Promise<void> {
  versions = await fetchJson<Versions>('/api/versions.json');
  latest = await fetchJson<Manifest>(`/api/${LATEST}/manifest.json`);
  manifests.clear();
  manifests.set(LATEST, latest);
  pages.clear();
  searches.clear();
  termFiles.clear();
  document.querySelectorAll('vite-error-overlay').forEach((overlay) => overlay.remove());
  const scroll = window.scrollY;
  await show(location.pathname, location.hash, true);
  window.scrollTo(0, scroll);
}

function pageMeta(page: PageData, t: Strings): HTMLElement {
  const box = element('div', undefined, 'page-meta');
  if (page.status || page.note || page.decision) {
    const line = element('p');
    if (page.status) line.append(element('span', t.status[page.status], `status status-${page.status}`));
    const decision = page.decision;
    if (decision?.date) line.append(element('span', decision.date, 'decision-date'));
    if (decision?.supersedes?.length) line.append(decisionNumbers(t.supersedes, decision.supersedes, t));
    if (decision?.supersededBy?.length) line.append(decisionNumbers(t.supersededBy, decision.supersededBy, t));
    if (page.note) line.append(page.note);
    box.append(line);
  }
  if (page.changes && manifest.previous) box.append(changesLine(page.changes, manifest.previous, t));
  if (page.translationOf && page.updated) {
    const line = element('p', `${t.translated(page.updated)} · `, 'meta-line');
    line.append(link(t.original, route({ ...current, lang: page.translationOf })));
    box.append(line);
  }

  const source = element('p', undefined, 'meta-line');
  const last = page.history[0];
  if (page.uncommittedChanges) source.append(element('span', t.uncommitted, 'uncommitted'), ' · ');
  else if (last) source.append(t.changed(last.date, last.commit), ' · ');
  source.append(link('Markdown', page.markdownUrl, true), ' · ', link('JSON', page.apiUrl, true));
  box.append(source);
  return box;
}

function versionBanner(t: Strings): HTMLElement {
  const banner = element('div', `${t.oldVersion(current.version)} `, 'version-banner');
  banner.append(link(t.toLatest, route({ ...current, version: LATEST })));
  return banner;
}

function pageHistory(page: PageData, t: Strings): HTMLElement {
  const section = element('section', undefined, 'page-history');
  section.id = 'page-history';
  if (page.history.length === 0 && !page.uncommittedChanges) return section;

  const list = element('ol');
  if (page.uncommittedChanges) list.append(element('li', t.uncommitted, 'uncommitted'));
  const versionOf = new Map(versions.versions.filter((v) => v.id !== LATEST && v.commit).map((v) => [v.commit!, v.id]));
  for (const change of page.history) {
    const item = element('li');
    const version = versionOf.get(change.commit);
    if (version) item.append(element('span', version, 'history-version'), ' ');
    item.append(
      element('span', change.date, 'history-date'),
      ' ',
      element('code', change.commit),
      ' ',
      element('span', change.subject),
      element('span', ` — ${change.author}`, 'history-author'),
    );
    list.append(item);
  }
  section.append(element('h2', t.history), list);
  return section;
}

/** A dot that marks what is new or changed since the previous version. */
function changeDot(status: 'added' | 'changed', t: Strings): HTMLElement {
  const dot = element('span', undefined, `change-dot change-${status}`);
  dot.title = status === 'added' ? t.addedMark : t.changedMark;
  return dot;
}

/** "Changed since v0.3.4 · Show the changes" under the title. */
function changesLine(changes: PageChanges, previous: string, t: Strings): HTMLElement {
  const line = element('p', undefined, 'meta-line changes-line');
  if (changes.status === 'added') {
    line.append(element('span', t.newSince(previous), 'change-text change-added'));
    return line;
  }
  line.append(element('span', t.changedSince(previous), 'change-text change-changed'), ' · ');
  const toggle = element('button', changesShown ? t.hideChanges : t.showChanges, 'link-button');
  toggle.type = 'button';
  toggle.addEventListener('click', () => {
    changesShown = !changesShown;
    void show(location.pathname, location.hash, true);
  });
  line.append(toggle);
  if (changes.removed?.length) line.append(' · ', t.removedSections(changes.removed.join(', ')));
  return line;
}

/** A small "changed" or "new" after the headings of the sections that changed. */
function markChangedHeadings(changes: PageChanges, t: Strings): void {
  if (changes.status === 'added') return;
  for (const { id, status } of changes.sections ?? []) {
    const heading = id ? document.getElementById(id) : ui.content.querySelector('h1');
    if (!heading || !ui.content.contains(heading)) continue;
    heading.append(element('span', status === 'added' ? t.addedMark : t.changedMark, `change-badge change-${status}`));
  }
}

/** The decision records of the language shown, by number. */
function decisionList(): Decision[] {
  return Object.entries(manifest.pages[current.lang] ?? {})
    .flatMap(([id, entry]) => (entry.decision ? [{ id, entry, info: entry.decision }] : []))
    .sort((a, b) => a.info.number - b.info.number);
}

function findDecision(number: number): Decision | undefined {
  return decisionList().find((d) => d.info.number === number);
}

function decisionHint(number: number): DecisionHint | null {
  const d = findDecision(number);
  if (!d) return null;
  const t = text();
  const status = d.entry.status ?? 'draft';
  const replacedBy = d.info.supersededBy ?? [];
  return {
    label: t.decision(number),
    title: d.entry.title,
    status,
    statusName: t.status[status],
    date: d.info.date,
    summary: d.info.summary,
    note: replacedBy.length ? `${t.supersededBy}: ${replacedBy.map((n) => t.decision(n)).join(', ')}` : undefined,
    url: d.entry.route,
  };
}

/** A link to a decision by its number: "Decision 7", or with its title. */
function decisionLink(number: number, t: Strings, withTitle = false): HTMLElement {
  const d = findDecision(number);
  const label = t.decision(number);
  if (!d) return element('span', label);
  return link(withTitle ? `${label}. ${d.entry.title}` : label, d.entry.route);
}

/** "Replaces: Decision 3, Decision 5" in the line under a record's title. */
function decisionNumbers(label: string, numbers: number[], t: Strings): HTMLElement {
  const span = element('span', `${label}: `, 'decision-relation');
  numbers.forEach((number, i) => {
    if (i > 0) span.append(', ');
    span.append(decisionLink(number, t));
  });
  return span;
}

/** "Decisions / Decision 7" above a record's title. */
function decisionEyebrow(info: DecisionInfo, t: Strings): HTMLElement {
  const line = element('p', undefined, 'decision-eyebrow');
  const index = manifest.pages[current.lang]?.[DECISIONS];
  if (index) line.append(link(index.navTitle ?? index.title, index.route), element('span', '/', 'separator'));
  line.append(element('span', t.decision(info.number)));
  const topic = manifest.decisionTopics?.find((x) => x.id === info.topic);
  if (topic) line.append(element('span', '·', 'separator'), element('span', topicTitle(topic)));
  return line;
}

function topicTitle(topic: DecisionTopic): string {
  return topic.title[current.lang] ?? topic.title[manifest.defaultLanguage] ?? topic.id;
}

function supersededBanner(numbers: number[], t: Strings): HTMLElement {
  const banner = element('div', `${t.supersededNotice} `, 'decision-banner');
  numbers.forEach((number, i) => {
    if (i > 0) banner.append(', ');
    banner.append(decisionLink(number, t, true));
  });
  return banner;
}

/** Under a record: the pages that rely on the decision, then the neighbouring decisions. */
function decisionFooter(page: PageData, t: Strings): HTMLElement {
  const section = element('section', undefined, 'decision-references');
  section.id = 'decision-references';
  section.append(element('h2', t.reflectedIn));
  const references = page.referencedBy ?? [];
  const described = references.filter((r) => r.decision === undefined);
  const decisions = references.filter((r) => r.decision !== undefined);
  if (references.length === 0) section.append(element('p', t.noReferences, 'decision-empty'));
  if (described.length > 0) {
    const list = element('ul');
    for (const reference of described) {
      const item = element('li');
      item.append(link(reference.title, reference.route));
      list.append(item);
    }
    section.append(list);
  }
  if (decisions.length > 0) {
    section.append(element('h3', t.referredBy));
    const list = element('ul');
    for (const reference of decisions) {
      const item = element('li');
      item.append(link(`${t.decision(reference.decision!)}. ${reference.title}`, reference.route));
      list.append(item);
    }
    section.append(list);
  }

  const all = decisionList();
  const index = all.findIndex((d) => d.id === page.id);
  const step = (d: Decision | undefined, label: string, className: string): HTMLElement => {
    if (!d) return element('span');
    const a = link('', d.entry.route);
    a.className = className;
    a.append(element('span', label, 'decision-nav-label'), element('span', `${d.info.number}. ${d.entry.title}`));
    return a;
  };
  const neighbours = element('nav', undefined, 'decision-nav');
  neighbours.append(
    step(index > 0 ? all[index - 1] : undefined, `← ${t.previous}`, 'decision-prev'),
    step(index >= 0 ? all[index + 1] : undefined, `${t.next} →`, 'decision-next'),
  );
  section.append(neighbours);
  return section;
}

/** The list on the decisions page: filters by status, then one row per decision. */
function renderDecisionList(holder: HTMLElement, t: Strings): void {
  const all = decisionList();
  const count = (filter: DecisionFilter) =>
    filter === 'all' ? all.length : all.filter((d) => d.entry.status === filter).length;
  const saved = storage.get('decision-filter') as DecisionFilter | null;
  let filter: DecisionFilter = saved && DECISION_FILTERS.includes(saved) && count(saved) > 0 ? saved : 'all';

  const filters = element('div', undefined, 'decision-filters');
  filters.setAttribute('role', 'group');
  const rows = element('div', undefined, 'decision-groups');
  const list = (items: Decision[]) => {
    const ol = element('ol', undefined, 'decision-rows');
    ol.append(...items.map((d) => decisionRow(d, t)));
    return ol;
  };
  // Records are grouped by topic, in the order the site lists the topics; versions written
  // before topics existed get one list, as before.
  const draw = () => {
    for (const button of filters.querySelectorAll<HTMLButtonElement>('button')) {
      button.setAttribute('aria-pressed', String(button.dataset.filter === filter));
    }
    const shown = filter === 'all' ? all : all.filter((d) => d.entry.status === filter);
    const topics = manifest.decisionTopics ?? [];
    if (topics.length === 0) {
      rows.replaceChildren(list(shown));
      return;
    }
    const groups: HTMLElement[] = [];
    for (const topic of topics) {
      const items = shown.filter((d) => d.info.topic === topic.id);
      if (items.length === 0) continue;
      const heading = element('h3', undefined, 'decision-topic');
      heading.append(element('span', topicTitle(topic)), element('span', String(items.length), 'decision-topic-count'));
      groups.push(heading, list(items));
    }
    const rest = shown.filter((d) => !topics.some((x) => x.id === d.info.topic));
    if (rest.length > 0) groups.push(list(rest));
    rows.replaceChildren(...groups);
  };
  for (const key of DECISION_FILTERS) {
    const n = count(key);
    if (key !== 'all' && n === 0) continue;
    const button = element('button', undefined, `decision-filter decision-filter-${key}`);
    button.type = 'button';
    button.dataset.filter = key;
    button.append(element('span', t.decisionFilters[key]), element('span', String(n), 'decision-filter-count'));
    button.addEventListener('click', () => {
      filter = key;
      storage.set('decision-filter', key);
      draw();
    });
    filters.append(button);
  }
  holder.replaceChildren(filters, rows);
  draw();
}

function decisionRow(d: Decision, t: Strings): HTMLLIElement {
  const status = d.entry.status ?? 'draft';
  const a = link('', d.entry.route);
  a.className = `decision-row is-${status}`;

  const main = element('span', undefined, 'decision-main');
  main.append(element('span', d.entry.title, 'decision-title'));
  if (d.info.summary) main.append(element('span', d.info.summary, 'decision-summary'));
  if (d.info.supersededBy?.length) {
    const replaced = d.info.supersededBy.map((n) => t.decision(n)).join(', ');
    main.append(element('span', `${t.supersededBy}: ${replaced}`, 'decision-summary'));
  }
  const side = element('span', undefined, 'decision-side');
  side.append(element('span', t.status[status], `status status-${status}`));
  if (d.info.date) side.append(element('span', d.info.date, 'decision-date'));

  a.append(element('span', String(d.info.number), 'decision-number'), main, side);
  const item = element('li');
  item.append(a);
  return item;
}

/** Links to decision records in page texts get a number badge and a hint (hints.ts). */
function markDecisionLinks(): void {
  const byRoute = new Map(decisionList().map((d) => [d.entry.route, d]));
  for (const a of ui.content.querySelectorAll<HTMLAnchorElement>('a[href]')) {
    if (a.closest('.decision-list, .decision-references, .decision-eyebrow, .page-meta, .decision-banner')) continue;
    const d = byRoute.get(new URL(a.href, location.href).pathname);
    if (!d) continue;
    a.classList.add('decision-link');
    a.dataset.decision = String(d.info.number);
    // The number follows the link's last word in one piece, so a line never breaks between them.
    // A draft is marked wherever it appears: its number here, an amber dot in the menu and in the list.
    const badge = element('span', String(d.info.number), `decision-badge${d.entry.status === 'draft' ? ' is-draft' : ''}`);
    const last = a.lastChild;
    const words = last?.nodeType === Node.TEXT_NODE ? (last.textContent ?? '') : '';
    const cut = words.search(/\S+\s*$/);
    if (last && cut >= 0) {
      const tail = element('span', words.slice(cut), 'decision-tail');
      tail.append(badge);
      last.textContent = words.slice(0, cut);
      a.append(tail);
    } else {
      a.append(badge);
    }
  }
}

function renderChrome(t: Strings): void {
  ui.search.placeholder = t.search;
  ui.search.setAttribute('aria-label', t.search);
  ui.menu.innerHTML = ICONS.menu;
  ui.menu.title = t.menu;
  ui.menu.setAttribute('aria-label', t.menu);
  renderThemeButton(currentTheme(), t);
}

function renderThemeButton(theme: Theme, t: Strings): void {
  ui.theme.innerHTML = ICONS[theme];
  ui.theme.title = t.theme[theme];
  ui.theme.setAttribute('aria-label', t.theme[theme]);
}

/** The versions, newest first, grouped by their first two numbers: v0.3, v0.2. */
function renderVersions(t: Strings): void {
  ui.version.setAttribute('aria-label', t.version);
  const option = (v: VersionInfo) => {
    const label = v.id === LATEST ? t.latest : [v.id, v.date, v.tagged ? t.milestone : ''].filter(Boolean).join(' · ');
    const item = element('option', `${t.version}: ${label}`);
    item.value = v.id;
    item.selected = v.id === current.version;
    return item;
  };
  const items: HTMLElement[] = [];
  const groups = new Map<string, HTMLOptGroupElement>();
  for (const v of versions.versions) {
    if (v.id === LATEST) {
      items.push(option(v));
      continue;
    }
    const key = v.id.split('.').slice(0, 2).join('.');
    let group = groups.get(key);
    if (!group) {
      group = document.createElement('optgroup');
      group.label = key;
      groups.set(key, group);
      items.push(group);
    }
    group.append(option(v));
  }
  ui.version.replaceChildren(...items);
}

function renderLanguages(): void {
  ui.languages.replaceChildren(
    ...manifest.languages.map((language) => {
      const samePage = manifest.pages[language.code]?.[current.id];
      const a = link(language.name, route({ ...current, lang: language.code, id: samePage ? current.id : 'index' }));
      a.lang = language.code;
      a.dataset.lang = language.code;
      if (language.code === current.lang) a.setAttribute('aria-current', 'true');
      return a;
    }),
  );
}

function renderSidebar(t: Strings): void {
  const pagesOfLang = manifest.pages[current.lang];
  const pageLink = (id: string, className: string): HTMLAnchorElement | null => {
    const page = pagesOfLang[id];
    if (!page) return null;
    const a = link(page.navTitle ?? page.title, page.route);
    a.className = className;
    if (id === current.id) a.setAttribute('aria-current', 'page');
    if (page.changes) a.append(changeDot(page.changes.status, t));
    return a;
  };

  const nav = element('nav');
  const changedPages = Object.values(pagesOfLang).filter((page) => page.changes).length;
  if (manifest.previous && changedPages > 0) {
    nav.append(element('div', t.changesSummary(changedPages, manifest.previous), 'changes-summary'));
  }
  for (const section of manifest.nav) {
    const group = element('div', undefined, 'nav-section');
    const title = section.title?.[current.lang] ?? section.title?.[manifest.defaultLanguage];
    const heading = section.index ? pageLink(section.index, 'nav-heading') : title ? element('div', title, 'nav-heading') : null;
    // A section without a heading shows its pages at the top level.
    const pageClass = heading ? 'nav-child' : 'nav-heading';
    if (heading) group.append(heading);
    const list = element('ul');
    for (const id of section.pages) {
      const a = pageLink(id, pageClass);
      if (!a) continue;
      const item = element('li');
      item.append(a);
      list.append(item);
    }
    if (section.index === DECISIONS && heading) {
      const decisions = decisionList();
      // Drafts wait for a decision, so the menu shows how many there are.
      const drafts = decisions.filter((d) => d.entry.status === 'draft').length;
      // Records are not in the menu outside their section, so the section shows that some changed.
      const inside = current.id === DECISIONS || current.id.startsWith(`${DECISIONS}/`);
      if (!inside && !heading.querySelector('.change-dot') && decisions.some((d) => d.entry.changes)) {
        heading.append(changeDot('changed', t));
      }
      if (drafts > 0) {
        const count = element('span', String(drafts), 'nav-count');
        count.title = t.drafts(drafts);
        count.setAttribute('aria-label', t.drafts(drafts));
        heading.append(count);
      }
      // Inside the section, the menu lists the decisions themselves, under their topics in the order
      // of the decisions page; versions written before topics existed get one list, as before.
      if (inside) {
        const item = (d: Decision): HTMLLIElement => {
          const status = d.entry.status ?? 'draft';
          const a = link('', d.entry.route);
          // A draft's title opens with the amber dot of its status, a replaced decision is struck through.
          a.className = `nav-child nav-decision is-${status}`;
          if (status === 'draft' || status === 'superseded') a.title = t.status[status];
          a.append(element('span', String(d.info.number), 'nav-number'), element('span', d.entry.navTitle ?? d.entry.title));
          if (d.entry.changes) a.lastElementChild!.append(changeDot(d.entry.changes.status, t));
          if (d.id === current.id) a.setAttribute('aria-current', 'page');
          const li = element('li');
          li.append(a);
          return li;
        };
        const topics = manifest.decisionTopics ?? [];
        for (const topic of topics) {
          const inTopic = decisions.filter((d) => d.info.topic === topic.id);
          if (inTopic.length > 0) list.append(element('li', topicTitle(topic), 'nav-topic'), ...inTopic.map(item));
        }
        list.append(...decisions.filter((d) => !topics.some((x) => x.id === d.info.topic)).map(item));
      }
    }
    group.append(list);
    nav.append(group);
  }

  const machine = element('div', undefined, 'nav-section machine');
  machine.append(element('div', t.forAgents, 'nav-title'));
  const list = element('ul');
  const files: [string, string][] = [
    ['llms.txt', '/llms.txt'],
    ['llms-full.txt', '/llms-full.txt'],
    ['API: versions.json', '/api/versions.json'],
    ['API: manifest.json', `/api/${current.version}/manifest.json`],
  ];
  for (const [label, href] of files) {
    const item = element('li');
    item.append(link(label, href, true));
    list.append(item);
  }
  machine.append(list);
  nav.append(machine);
  ui.sidebar.replaceChildren(nav);

  // A long section, such as the decisions, can hide the current page: bring it into view.
  const active = ui.sidebar.querySelector<HTMLElement>('[aria-current="page"]');
  const box = ui.sidebar;
  if (active && (active.offsetTop < box.scrollTop || active.offsetTop + active.offsetHeight > box.scrollTop + box.clientHeight)) {
    box.scrollTop = active.offsetTop - box.clientHeight / 3;
  }
}

function renderToc(page: PageData, t: Strings): void {
  const headings = page.headings.filter((h) => h.level === 2 || h.level === 3);
  if (page.decision) headings.push({ level: 2, id: 'decision-references', text: t.reflectedIn });
  if (headings.length === 0) {
    ui.toc.replaceChildren();
    return;
  }
  const changed = new Map((page.changes?.sections ?? []).map((s) => [s.id, s.status]));
  const list = element('ul');
  for (const heading of headings) {
    const item = element('li', undefined, `toc-level-${heading.level}`);
    const a = link(heading.text, `#${heading.id}`);
    const kind = changed.get(heading.id);
    if (kind) a.append(changeDot(kind, t));
    item.append(a);
    list.append(item);
  }
  ui.toc.replaceChildren(element('div', t.onThisPage, 'toc-title'), list);
}

function scrollToHash(hash: string): void {
  const id = decodeURIComponent(hash.replace(/^#/, ''));
  const target = id ? document.getElementById(id) : null;
  if (target) target.scrollIntoView();
  else window.scrollTo(0, 0);
}

function currentTheme(): Theme {
  const saved = storage.get('theme');
  return saved === 'light' || saved === 'dark' ? saved : 'auto';
}

function applyTheme(theme: Theme): void {
  if (theme === 'auto') delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
}

async function updateResults(): Promise<void> {
  const query = ui.search.value.trim();
  if (!query) return hideResults();
  const { version, lang } = current;
  const key = `${version}/${lang}`;
  let search = searches.get(key);
  if (!search) {
    search = await Search.load(`/api/${version}/search/${lang}.json`, lang);
    searches.set(key, search);
  }
  if (ui.search.value.trim() !== query || key !== `${current.version}/${current.lang}`) return;

  const numbers = new Map(decisionList().map((d) => [d.entry.route, d.info.number]));
  const items = search.query(query).map((result) => {
    const a = link(result.title, result.heading ? `${result.route}#${result.heading.id}` : result.route);
    a.replaceChildren(element('span', result.title, 'result-title'));
    const number = numbers.get(result.route);
    if (number !== undefined) a.prepend(element('span', String(number), 'result-number'));
    if (result.heading) a.append(element('span', result.heading.text, 'result-heading'));
    const item = element('li');
    item.append(a);
    return item;
  });
  ui.results.replaceChildren(...(items.length > 0 ? items : [element('li', text().noResults, 'empty')]));
  ui.results.hidden = false;
}

function hideResults(): void {
  ui.results.hidden = true;
}

// Links inside the site open without reloading the page. Files for agents and programs
// (/md/, /api/, llms.txt) open as they are.
document.addEventListener('click', (event) => {
  const target = event.target as Element;
  if (!target.closest('.search')) hideResults();

  const a = target.closest('a');
  if (!a || event.defaultPrevented || event.button !== 0) return;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || a.target) return;
  const href = a.getAttribute('href');
  if (!href || href.startsWith('#')) return;
  const url = new URL(href, location.href);
  if (url.origin !== location.origin || /^\/(api|md)\/|^\/llms/.test(url.pathname)) return;

  if (a.dataset.lang) storage.set('lang', a.dataset.lang);
  event.preventDefault();
  navigate(url.pathname + url.hash);
});

// Back and forward. Moving between headings of the same page is left to the browser.
window.addEventListener('popstate', () => {
  if (location.pathname !== shownPath) void show(location.pathname, location.hash);
});

ui.version.addEventListener('change', async () => {
  const version = ui.version.value;
  const target = await loadManifest(version);
  if (!target) return;
  const lang = target.pages[current.lang] ? current.lang : target.defaultLanguage;
  const id = target.pages[lang][current.id] ? current.id : 'index';
  navigate(route({ version, lang, id }));
});

ui.search.addEventListener('input', () => void updateResults());
ui.search.addEventListener('focus', () => void updateResults());
ui.search.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') {
    hideResults();
    ui.search.blur();
  } else if (event.key === 'Enter') {
    ui.results.querySelector('a')?.click();
  }
});

ui.theme.addEventListener('click', () => {
  const order: Theme[] = ['auto', 'light', 'dark'];
  const next = order[(order.indexOf(currentTheme()) + 1) % order.length];
  storage.set('theme', next);
  applyTheme(next);
  renderThemeButton(next, text());
});

ui.menu.addEventListener('click', () => document.body.classList.toggle('menu-open'));

new ResizeObserver(() => {
  document.documentElement.style.setProperty('--topbar-height', `${ui.topbar.offsetHeight}px`);
}).observe(ui.topbar);

// In development the dev server rebuilds the data whenever the docs change (see vite.config.ts).
if (import.meta.hot) {
  import.meta.hot.on('docs:updated', () => void reloadData());
}

applyTheme(currentTheme());
await show(location.pathname, location.hash);
