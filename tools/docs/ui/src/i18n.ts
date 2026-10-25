export type Status = 'draft' | 'accepted' | 'partial' | 'living' | 'superseded';
/** What the list of decisions can be narrowed to. */
export type DecisionFilter = 'all' | 'draft' | 'accepted' | 'superseded';
export type Theme = 'auto' | 'light' | 'dark';

/** Interface texts in one language. Page texts come from the documentation itself. */
export interface Strings {
  search: string;
  noResults: string;
  onThisPage: string;
  menu: string;
  theme: Record<Theme, string>;
  status: Record<Status, string>;
  translated: (date: string) => string;
  original: string;
  notFound: string;
  notFoundHint: string;
  version: string;
  latest: string;
  oldVersion: (version: string) => string;
  toLatest: string;
  history: string;
  uncommitted: string;
  changed: (date: string, commit: string) => string;
  forAgents: string;
  /** Link from a term's hint to its card in the glossary. */
  termMore: string;
  /** Decision records. */
  decision: (number: number) => string;
  decisionFilters: Record<DecisionFilter, string>;
  /** Hint on the menu's count of drafts. */
  drafts: (count: number) => string;
  supersedes: string;
  supersededBy: string;
  supersededNotice: string;
  reflectedIn: string;
  referredBy: string;
  noReferences: string;
  openDecision: string;
  previous: string;
  next: string;
  /** Changes against the previous version. */
  changedSince: (version: string) => string;
  newSince: (version: string) => string;
  showChanges: string;
  hideChanges: string;
  removedSections: (titles: string) => string;
  changedMark: string;
  addedMark: string;
  changesSummary: (count: number, version: string) => string;
  milestone: string;
}

export const strings: Record<string, Strings> = {
  en: {
    search: 'Search',
    noResults: 'Nothing found',
    onThisPage: 'On this page',
    menu: 'Menu',
    theme: { auto: 'Theme: system', light: 'Theme: light', dark: 'Theme: dark' },
    status: {
      draft: 'Draft',
      accepted: 'Accepted',
      partial: 'Partially accepted',
      living: 'Living list',
      superseded: 'Superseded',
    },
    translated: (date) => `Translated from English, updated ${date}`,
    original: 'Original',
    notFound: 'Page not found',
    notFoundHint: 'There is no such page.',
    version: 'Version',
    latest: 'latest',
    oldVersion: (version) => `You are reading version ${version}.`,
    toLatest: 'Go to the latest version',
    history: 'Page history',
    uncommitted: 'Not committed yet',
    changed: (date, commit) => `Changed ${date} (${commit})`,
    forAgents: 'For AI agents and programs',
    termMore: 'Open in the glossary',
    decision: (number) => `Decision ${number}`,
    decisionFilters: { all: 'All', draft: 'Drafts', accepted: 'Accepted', superseded: 'Superseded' },
    drafts: (count) => (count === 1 ? '1 draft' : `${count} drafts`),
    supersedes: 'Replaces',
    supersededBy: 'Replaced by',
    supersededNotice: 'This decision has been replaced:',
    reflectedIn: 'Reflected in',
    referredBy: 'Decisions that refer to it',
    noReferences: 'No page links to this decision yet.',
    openDecision: 'Open the decision',
    previous: 'Previous',
    next: 'Next',
    changedSince: (version) => `Changed since ${version}`,
    newSince: (version) => `New since ${version}`,
    showChanges: 'Show the changes',
    hideChanges: 'Hide the changes',
    removedSections: (titles) => `Removed sections: ${titles}`,
    changedMark: 'changed',
    addedMark: 'new',
    changesSummary: (count, version) => `Changed since ${version}: ${count} ${count === 1 ? 'page' : 'pages'}`,
    milestone: 'milestone',
  },
  ru: {
    search: 'Поиск',
    noResults: 'Ничего не найдено',
    onThisPage: 'На этой странице',
    menu: 'Меню',
    theme: { auto: 'Тема: как в системе', light: 'Тема: светлая', dark: 'Тема: тёмная' },
    status: {
      draft: 'Черновик',
      accepted: 'Принято',
      partial: 'Принято частично',
      living: 'Живой список',
      superseded: 'Заменено',
    },
    translated: (date) => `Перевод с английского, обновлён ${date}`,
    original: 'Оригинал',
    notFound: 'Страница не найдена',
    notFoundHint: 'Такой страницы нет.',
    version: 'Версия',
    latest: 'текущая',
    oldVersion: (version) => `Вы читаете версию ${version}.`,
    toLatest: 'Перейти к текущей версии',
    history: 'История изменений',
    uncommitted: 'Ещё не закоммичено',
    changed: (date, commit) => `Изменено ${date} (${commit})`,
    forAgents: 'Для ИИ-агентов и программ',
    termMore: 'Открыть в терминологии',
    decision: (number) => `Решение ${number}`,
    decisionFilters: { all: 'Все', draft: 'Черновики', accepted: 'Принятые', superseded: 'Заменённые' },
    drafts: (count) => {
      const forms = { one: 'черновик', few: 'черновика', many: 'черновиков', other: 'черновика' };
      return `${count} ${forms[new Intl.PluralRules('ru').select(count) as keyof typeof forms] ?? forms.many}`;
    },
    supersedes: 'Заменяет',
    supersededBy: 'Заменено',
    supersededNotice: 'Это решение заменено:',
    reflectedIn: 'Где отражено',
    referredBy: 'На него ссылаются решения',
    noReferences: 'Пока ни одна страница не ссылается на это решение.',
    openDecision: 'Открыть решение',
    previous: 'Предыдущее',
    next: 'Следующее',
    changedSince: (version) => `Изменено относительно ${version}`,
    newSince: (version) => `Новая страница относительно ${version}`,
    showChanges: 'Показать правки',
    hideChanges: 'Скрыть правки',
    removedSections: (titles) => `Удалены разделы: ${titles}`,
    changedMark: 'изменено',
    addedMark: 'новое',
    changesSummary: (count, version) => {
      const forms = { one: 'страница', few: 'страницы', many: 'страниц', other: 'страницы' };
      const form = forms[new Intl.PluralRules('ru').select(count) as keyof typeof forms] ?? forms.many;
      return `Изменено относительно ${version}: ${count} ${form}`;
    },
    milestone: 'веха',
  },
  hi: {
    search: 'खोजें',
    noResults: 'कुछ नहीं मिला',
    onThisPage: 'इस पेज पर',
    menu: 'मेनू',
    theme: { auto: 'थीम: सिस्टम जैसी', light: 'थीम: हल्की', dark: 'थीम: गहरी' },
    status: {
      draft: 'मसौदा',
      accepted: 'स्वीकृत',
      partial: 'आंशिक रूप से स्वीकृत',
      living: 'लगातार अद्यतन सूची',
      superseded: 'बदला गया',
    },
    translated: (date) => `अंग्रेज़ी संस्करण का अनुवाद, अद्यतन: ${date}`,
    original: 'मूल संस्करण',
    notFound: 'पेज नहीं मिला',
    notFoundHint: 'ऐसा कोई पेज नहीं है।',
    version: 'संस्करण',
    latest: 'नवीनतम',
    oldVersion: (version) => `आप संस्करण ${version} पढ़ रहे हैं।`,
    toLatest: 'नवीनतम संस्करण पर जाएँ',
    history: 'बदलावों का इतिहास',
    uncommitted: 'अभी कमिट नहीं हुआ',
    changed: (date, commit) => `बदला गया: ${date} (${commit})`,
    forAgents: 'AI एजेंटों और प्रोग्रामों के लिए',
    termMore: 'शब्दावली में खोलें',
    decision: (number) => `निर्णय ${number}`,
    decisionFilters: { all: 'सभी', draft: 'मसौदे', accepted: 'स्वीकृत', superseded: 'बदले गए' },
    drafts: (count) => (count === 1 ? '1 मसौदा' : `${count} मसौदे`),
    supersedes: 'इसकी जगह लेता है',
    supersededBy: 'इसकी जगह आया',
    supersededNotice: 'इस निर्णय की जगह दूसरा निर्णय आ गया है:',
    reflectedIn: 'कहाँ दर्ज है',
    referredBy: 'इसका हवाला देने वाले निर्णय',
    noReferences: 'अभी कोई पृष्ठ इस निर्णय से नहीं जुड़ा है।',
    openDecision: 'निर्णय खोलें',
    previous: 'पिछला',
    next: 'अगला',
    changedSince: (version) => `${version} के बाद बदला गया`,
    newSince: (version) => `${version} के बाद नया पृष्ठ`,
    showChanges: 'बदलाव दिखाएँ',
    hideChanges: 'बदलाव छिपाएँ',
    removedSections: (titles) => `हटाए गए खंड: ${titles}`,
    changedMark: 'बदला गया',
    addedMark: 'नया',
    changesSummary: (count, version) => `${version} के बाद बदले पृष्ठ: ${count}`,
    milestone: 'पड़ाव',
  },
};
