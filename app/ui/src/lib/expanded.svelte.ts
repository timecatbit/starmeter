// What the user opened stays open (用户：展开会话或项目后不要再折回去): 查看更多 on the models', projects' and sessions'
// lists, each grouping by its own, and every session opened. They used to live in the lists' components and went
// with them, so switching tabs, opening a page and coming back, or a fresh reading folded them up again. Kept in the
// web view's storage, so a reopened panel or a restarted app has them as they were.

const KEY = 'starmeter.expanded';
/** Sessions are many over a month: only the last ones opened are kept. */
const SESSIONS_KEPT = 200;

type Saved = { lists: string[]; sessions: string[] };

function load(): Saved {
  try {
    const saved = JSON.parse((typeof localStorage !== 'undefined' && localStorage.getItem(KEY)) || '{}');
    const strings = (value: unknown) => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
    return { lists: strings(saved.lists), sessions: strings(saved.sessions) };
  } catch {
    return { lists: [], sessions: [] };
  }
}

export const expanded = $state<Saved>(load());

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify({ lists: expanded.lists, sessions: expanded.sessions.slice(-SESSIONS_KEPT) }));
  } catch {
    // A web view without storage still keeps them for as long as it runs.
  }
}

/** A list shown whole: `model`, `project` or `sessions`. */
export const listOpen = (name: string) => expanded.lists.includes(name);

export function openList(name: string) {
  if (listOpen(name)) return;
  expanded.lists = [...expanded.lists, name];
  save();
}

export const sessionOpen = (key: string) => expanded.sessions.includes(key);

export function toggleSession(key: string) {
  expanded.sessions = sessionOpen(key) ? expanded.sessions.filter((known) => known !== key) : [...expanded.sessions, key].slice(-SESSIONS_KEPT);
  save();
}
