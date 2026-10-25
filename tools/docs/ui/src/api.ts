// Types of the JSON files under /api/, written by the docs engine (tools/docs/engine/src/build.rs).

import type { Status } from './i18n';

export const LATEST = 'latest';

export interface Heading {
  level: number;
  id: string;
  text: string;
}

export interface Commit {
  commit: string;
  date: string;
  author: string;
  subject: string;
}

export interface VersionInfo {
  id: string;
  commit?: string;
  date?: string;
  /** A milestone: the number comes from a Git tag. */
  tagged?: boolean;
  /** The version this one is compared with. */
  previous?: string;
  manifest: string;
}

/** A section that is new or changed since the previous version; `''` is the part above the first heading. */
export interface SectionChange {
  id: string;
  status: 'added' | 'changed';
}

/** How a page differs from the same page in the previous version. */
export interface PageChanges {
  status: 'added' | 'changed';
  sections?: SectionChange[];
  /** Titles of sections that the previous version had. */
  removed?: string[];
}

/** `/api/versions.json`: newest first, `latest` is always the first. */
export interface Versions {
  schemaVersion: number;
  versions: VersionInfo[];
}

export interface Language {
  code: string;
  name: string;
}

/** A menu section. Its `index` page, if any, gives the section title and is where the title links to. */
export interface NavSection {
  index?: string;
  title?: Record<string, string>;
  pages: string[];
}

interface Meta {
  navTitle?: string;
  status?: Status;
  note?: string;
  translationOf?: string;
  updated?: string;
}

/** What a decision record adds to its page: `decisions/007-….md` is decision 7. */
export interface DecisionInfo {
  number: number;
  /** When the decision was made or, for a draft, proposed. */
  date?: string;
  /** The decision in a sentence or two: the paragraph under the record's title. */
  summary?: string;
  /** Decisions this one replaces. */
  supersedes?: number[];
  /** Decisions that replace this one. */
  supersededBy?: number[];
  /** The topic: one of the manifest's `decisionTopics`. */
  topic?: string;
}

/** A topic of decision records; the decisions page groups records by topic. */
export interface DecisionTopic {
  id: string;
  /** Language → title. */
  title: Record<string, string>;
}

/** A page that links to a decision record. */
export interface Reference {
  id: string;
  title: string;
  route: string;
  /** The number, when the linking page is a decision record too. */
  decision?: number;
}

export interface PageEntry extends Meta {
  title: string;
  route: string;
  apiUrl: string;
  markdownUrl: string;
  lastChange?: Commit;
  uncommittedChanges: boolean;
  decision?: DecisionInfo;
  changes?: PageChanges;
}

/** `/api/<version>/manifest.json`. */
export interface Manifest {
  schemaVersion: number;
  version: string;
  name: string;
  summary: string;
  defaultLanguage: string;
  languages: Language[];
  nav: NavSection[];
  /** Topics of decision records, in display order; versions before topics have none. */
  decisionTopics?: DecisionTopic[];
  commit?: string;
  /** The version the pages' `changes` are counted from. */
  previous?: string;
  /** Language → page id → page. */
  pages: Record<string, Record<string, PageEntry>>;
}

/** `/api/<version>/pages/<lang>/<id>.json`. */
export interface PageData extends Meta {
  schemaVersion: number;
  version: string;
  lang: string;
  id: string;
  title: string;
  route: string;
  apiUrl: string;
  markdownUrl: string;
  /** Language → API URL of the same page. */
  translations: Record<string, string>;
  headings: Heading[];
  history: Commit[];
  uncommittedChanges: boolean;
  markdown: string;
  html: string;
  decision?: DecisionInfo;
  /** Decision records only: the pages of the same language that link to the record. */
  referencedBy?: Reference[];
  changes?: PageChanges;
}

/** A glossary term, for the hints over terms in page texts. */
export interface TermInfo {
  name: string;
  code: string[];
  kind: string;
  /** `accepted`, `proposed` or `open`. */
  status: string;
  statusName: string;
  /** The definition as HTML. */
  html: string;
  /** The term's card on the glossary page. */
  url: string;
}

/** `/api/<version>/terms/<lang>.json`; versions without a glossary have none. */
export interface TermsFile {
  schemaVersion: number;
  version: string;
  lang: string;
  /** Route of the glossary page. */
  glossary: string;
  /** Term id → the term. */
  terms: Record<string, TermInfo>;
}

export async function fetchJson<T>(url: string): Promise<T> {
  // Revalidate every time: the data changes whenever the docs are rebuilt.
  const response = await fetch(url, { cache: 'no-cache' });
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  return (await response.json()) as T;
}
