// The panel's data, as the engine hands it over: the 5.x panel snapshot (PanelSnapshot.swift) and the dashboard
// (DashboardPayload.swift), field for field. Optional fields are ones the engine leaves out when they don't apply.

/** One row: a homepage row, a page's row, a module in the grid or a setting. Rows nest through `children`. */
export interface Entry {
  label: string;
  children: Entry[];
  separator?: boolean;
  /** Base64 of an SVG or PNG; `template` means a one-colour mark drawn in the text colour. */
  image?: string;
  template?: boolean;
  /** An SF Symbols name, drawn from the panel's own icon set. */
  symbol?: string;
  /** A small group title. */
  section?: boolean;
  /** The value draws as an accent pill (工具更新). */
  attention?: boolean;
  checked?: boolean;
  radio?: boolean;
  detail?: string;
  /** The detail is a path or address, shown exactly as written in every language. */
  verbatim?: boolean;
  value?: string;
  /** Shorter forms of the value, for when the label needs the room. */
  fallbacks?: string[];
  usageLevel?: 'normal' | 'warning' | 'critical';
  /** The share used (0–1) for the row's bar when the value isn't 已用 X%: a plan on its reserve, whose value is what is
   *  left to spend (余 41,554 Credits). */
  share?: number;
  /** That bar's own level (a window used up is critical, red), the value keeping the reserve's orange. */
  shareLevel?: 'normal' | 'warning' | 'critical';
  /** A plan with more than one limit: a bar for each window shown, in place of the single bar of the tightest, its name
   *  and share (percent) under it. `place` 0 and 1 are the first line's left (five hours) and right (the week), 2 and 3
   *  the second line's; `group`, on a line's first bar, is the model group the line is for (Gemini 订阅's), and `marks`
   *  that group's logos, drawn at the end of the line's names. */
  gauges?: { label: string; used: number; level: 'normal' | 'warning' | 'critical'; place: number; group?: string; marks?: { image: string; template: boolean }[] }[];
  statusLight?: 'new' | 'ok' | 'issue' | 'unknown';
  /** What tells a row from siblings of the same name (a session's ID). */
  key?: string;
  href?: string;
  action?: string[];
  /** Sent once when the row's page opens (refresh-source <key>), at most every 5 seconds. */
  onOpenAction?: string[];
  /** A homepage row's module: the panel can drag it to another place in its group (move-module). */
  module?: string;
  /** In English, what the row said in Chinese (i18n.svelte.ts): the panel's logic reads rows by their Chinese wording. */
  source?: { label: string; value?: string; detail?: string };
}

export interface ModelUsage {
  name: string;
  tokens: number;
  share: number;
  credits?: number;
  priceNote?: string;
}

export interface CountedUsage {
  cost: number | null;
  tokens: number;
  /** The output tokens of the counted part, as the engine matched them (not a share of the card's). */
  output?: number;
  requests: number;
  unpriced: number;
  cache: number | null;
  /** The cache-eligible input the share is of. */
  cacheInputs?: number;
  history: number[];
}

/** A Harness card: one tool, or a model or project when the cards are regrouped. */
export interface ProviderUsage {
  id: string;
  name: string;
  image?: string;
  template?: boolean;
  symbol?: string;
  subtitle: string;
  status: string;
  available: boolean;
  cost: number | null;
  credits: number | null;
  tokens: number;
  requests: number;
  output: number;
  cache: number | null;
  /** The cache-eligible input the share is of (from the engine; tests may leave it out). */
  cacheInputs?: number;
  coverage: number | null;
  sessions: number;
  unpriced: number;
  summaryIncluded?: boolean;
  /** What Summary counts when a gateway counted the rest. */
  counted?: CountedUsage;
  models: ModelUsage[];
  clients?: ModelUsage[] | null;
  history: number[];
  /** A model or project card takes the colour of the tool or brand it belongs to. */
  accent?: string | null;
  priceNote?: string | null;
  serviceNote?: string | null;
  /** The service in trouble, for the tool's page: its name, the note, what is wrong (a line each) and where to read
   *  about it (the incident's page, or the status page). */
  serviceStatus?: { name: string; note: string; detail: string | null; link: string } | null;
}

export interface UsageHeatmap {
  columns: number;
  rowLabels?: string[];
  labels: string[];
  tokens: number[];
  requests: number[];
  costs: number[];
  unpriced: number[];
  /** The last `future` squares are still to come. */
  future: number;
  hint: string;
  status?: string;
  notice?: string;
  streak?: string | null;
}

export interface Grouping {
  key: string;
  label: string;
}

export interface MoneyUnit {
  code: string;
  symbol: string;
  rate: number;
  note?: string;
}

export interface Dashboard {
  date: string;
  dates: string[];
  providers: ProviderUsage[];
  rangeKey: string;
  rangeLabel: string;
  chartUnit: string;
  chartSpan: string;
  period?: string[];
  chartStatus: string;
  updated?: string;
  /** When the range's figures were read (seconds): a range picked shows once its reading is this fresh. */
  fetchedAt?: number | null;
  chartPositions?: number[] | null;
  heatmap?: UsageHeatmap | null;
  showSummary?: boolean;
  allowCollapse?: boolean;
  expandedProviders?: string[];
  grouping?: string;
  groupings?: Grouping[];
  /** Regrouped cards (model, project); null when the cards are per tool. */
  cards?: ProviderUsage[] | null;
  currency?: MoneyUnit | null;
  groupNote?: string | null;
  /** 多台电脑合并: how many other computers' figures are in this range. */
  devices?: number;
}

export interface Project {
  name: string;
  url: string;
}

/** The display preferences a saved action confirms, so an older refresh never undoes a newer choice. */
export interface PresentationPreferences {
  revision: number;
  visible: string[];
  appearance: string;
  theme?: string;
  allowCollapse: boolean;
  expandedProviders: string[];
  /** The homepage rows' order the user dragged them into; empty is the module list's. */
  homeOrder?: string[];
}

export interface Snapshot {
  schema?: number;
  project?: Project;
  menuBarText?: string;
  pinInterval?: number;
  dashboard?: Dashboard | null;
  entries: Entry[];
  rangeOptions?: Entry[];
  updateCount?: number;
  moduleOptions?: Entry[];
  appearance?: string;
  theme?: string;
  preferences?: PresentationPreferences;
}

/** What an action left behind: done, failed (with why) or cancelled (a dialog was dismissed). */
export interface ActionReceipt {
  status: 'done' | 'failed' | 'cancelled';
  message?: string;
  preferences?: PresentationPreferences;
}

/** Facts about the host the panel runs in, from the shell. */
export interface ShellInfo {
  platform: 'macos' | 'windows' | 'linux' | 'browser';
  /** standalone: the app's own star or tray icon; swiftbar: launched by the SwiftBar plugin, no icon of its own. */
  mode: 'standalone' | 'swiftbar' | 'browser';
  version: string;
  /** The most the panel may grow before it scrolls: the screen's working height less a margin. */
  maxHeight: number;
  /** Opened from the star's right-click menu: the module list is the whole panel. */
  modules: boolean;
}

/** A question the core asks while an action runs, answered in the panel: confirm (true/false), text (the line typed),
 *  choice (one option's value); a notice only tells (what went wrong) and waits for nothing. */
export interface Question {
  /** A number for the app's own core, a string for the SwiftBar plugin's (answered through a file). */
  id: number | string;
  kind: 'confirm' | 'text' | 'choice' | 'notice';
  title: string;
  detail?: string;
  accept?: string;
  value?: string;
  hidden?: boolean;
  options?: [string, string][];
}
