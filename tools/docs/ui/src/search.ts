import { fetchJson, type Heading } from './api';

/** One page in `/api/<version>/search/<lang>.json`. */
interface SearchEntry {
  id: string;
  title: string;
  route: string;
  headings: Heading[];
  text: string;
}

export interface SearchResult {
  title: string;
  route: string;
  /** The heading that matches the whole query, if there is one. */
  heading?: Heading;
}

interface Indexed {
  entry: SearchEntry;
  title: string;
  headings: string;
  text: string;
}

/** Full-text search over one language. Words are split with `Intl.Segmenter`, so Hindi works too. */
export class Search {
  private readonly segmenter: Intl.Segmenter;

  private constructor(
    private readonly lang: string,
    private readonly items: Indexed[],
  ) {
    this.segmenter = new Intl.Segmenter(lang, { granularity: 'word' });
  }

  static async load(url: string, lang: string): Promise<Search> {
    const entries = await fetchJson<SearchEntry[]>(url);
    const lower = (text: string) => text.toLocaleLowerCase(lang);
    return new Search(
      lang,
      entries.map((entry) => ({
        entry,
        title: lower(entry.title),
        headings: lower(entry.headings.map((h) => h.text).join('\n')),
        text: lower(entry.text),
      })),
    );
  }

  query(input: string, limit = 8): SearchResult[] {
    const words = Array.from(this.segmenter.segment(input.toLocaleLowerCase(this.lang)))
      .filter((s) => s.isWordLike)
      .map((s) => s.segment);
    if (words.length === 0) return [];

    const found: { score: number; result: SearchResult }[] = [];
    for (const item of this.items) {
      let score = 0;
      for (const word of words) {
        const points = item.title.includes(word) ? 10 : item.headings.includes(word) ? 4 : item.text.includes(word) ? 1 : 0;
        if (points === 0) {
          score = 0;
          break;
        }
        score += points;
      }
      if (score === 0) continue;
      const heading = item.entry.headings.find(
        (h) => h.level > 1 && words.every((word) => h.text.toLocaleLowerCase(this.lang).includes(word)),
      );
      found.push({ score, result: { title: item.entry.title, route: item.entry.route, heading } });
    }
    return found
      .sort((a, b) => b.score - a.score)
      .slice(0, limit)
      .map((f) => f.result);
  }
}
