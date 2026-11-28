// What the user opened stays open until they fold it (用户：展开之后就是不能再折叠回去了): 查看更多 on the models',
// projects' and sessions' lists, each grouping by its own, folded again with 收起 at the list's end; and every session
// opened. They used to live in the lists' components and went
// with them, so switching tabs, opening a page and coming back, or a fresh reading folded them up again. Kept in the
// web view's storage, so a reopened panel or a restarted app has them as they were.

const KEY = 'starmeter.expanded';
/** Sessions are many over a month: only the last ones opened are kept. */
const SESSIONS_KEPT = 200;

type Saved = { lists: string[]; sessions: string[]; folded: string[] };

function load(): Saved {
  try {
    const saved = JSON.parse((typeof localStorage !== 'undefined' && localStorage.getItem(KEY)) || '{}');
    const strings = (value: unknown) => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
    return { lists: strings(saved.lists), sessions: strings(saved.sessions), folded: strings(saved.folded) };
  } catch {
    return { lists: [], sessions: [], folded: [] };
  }
}

export const expanded = $state<Saved>(load());

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify({
      lists: expanded.lists,
      sessions: expanded.sessions.slice(-SESSIONS_KEPT),
      folded: expanded.folded,
    }));
  } catch {
    // A web view without storage still keeps them for as long as it runs.
  }
}

/** A list shown whole: `model`, `project`, `sessions`, or `feed` (实时's 动态, on the panel and on 网页版). */
export const listOpen = (name: string) => expanded.lists.includes(name);

export function openList(name: string) {
  if (listOpen(name)) return;
  expanded.lists = [...expanded.lists, name];
  save();
}

/** 收起: back to its first five. */
export function closeList(name: string) {
  if (!listOpen(name)) return;
  expanded.lists = expanded.lists.filter((known) => known !== name);
  save();
}

/** Homepage sections folded independently (e.g. `home-plans`, `home-apis`, `home-services`), persisted across restarts. */
export const sectionFolded = (name: string) => expanded.folded.includes(name);

export function foldSection(name: string) {
  if (sectionFolded(name)) return;
  expanded.folded = [...expanded.folded, name];
  save();
}

export function unfoldSection(name: string) {
  if (!sectionFolded(name)) return;
  expanded.folded = expanded.folded.filter((known) => known !== name);
  save();
}

export function toggleSection(name: string) {
  if (sectionFolded(name)) unfoldSection(name); else foldSection(name);
}

export const sessionOpen = (key: string) => expanded.sessions.includes(key);

export function toggleSession(key: string) {
  expanded.sessions = sessionOpen(key) ? expanded.sessions.filter((known) => known !== key) : [...expanded.sessions, key].slice(-SESSIONS_KEPT);
  save();
}
