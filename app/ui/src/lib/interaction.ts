// Actions in flight, and the snapshot as the panel shows it meanwhile: a choice shows at once, and a saved preference
// revision wins over an older data refresh. The 5.x panel's PanelInteraction.
import { automatic, choices, isPreference } from './detail';
import type { Entry, PresentationPreferences, Snapshot } from './types';

const same = (a: string[] | undefined, b: string[]) => !!a && a.length === b.length && a.every((x, i) => x === b[i]);
const lastWins = (action: string[]) => action[0] === 'set-range' || action[0] === 'set-group';

export class PanelInteraction {
  source: Snapshot | null = null;
  confirmed: PresentationPreferences | null = null;
  active: string[] | null = null;
  queued: string[][] = [];

  /** 'start' runs it now, 'queued' after the one running; repeated and automatic ones are dropped while busy. */
  submit(action: string[]): 'start' | 'queued' | 'ignored' {
    if (!action.length) return 'ignored';
    if (!this.active) { this.active = action; return 'start'; }
    if (automatic(action) || this.queued.length >= 64) return 'ignored';
    // A range or a grouping: only the last pick counts. One still waiting gives way to it, and picking the one already
    // on its way again (away and back) leaves nothing to do after it.
    if (lastWins(action)) {
      this.queued = this.queued.filter((q) => q[0] !== action[0]);
      if (same(this.active, action)) return 'ignored';
      this.queued.push(action);
      return 'queued';
    }
    if (!isPreference(action) && (same(this.active, action) || this.queued.some((q) => same(q, action)))) return 'ignored';
    this.queued.push(action);
    return 'queued';
  }

  finish(preferences?: PresentationPreferences) {
    if (preferences && preferences.revision >= (this.confirmed?.revision ?? -1)) this.confirmed = preferences;
    this.active = null;
  }

  next(): string[] | null {
    if (this.active || !this.queued.length) return null;
    this.active = this.queued.shift()!;
    return this.active;
  }

  displayed(): Snapshot | null {
    if (!this.source) return null;
    const value: Snapshot = structuredClone(this.source);
    let authoritative = value.preferences ?? null;
    if (this.confirmed && this.confirmed.revision > (authoritative?.revision ?? -1)) authoritative = this.confirmed;
    if (authoritative) applySettings(authoritative, value);
    for (const action of [...(this.active ? [this.active] : []), ...this.queued]) { applyPreference(action, value); applyChoice(action, value); }
    return value;
  }
}

function applyChoice(action: string[], value: Snapshot) {
  if (!choices.has(action[0])) return;
  const walk = (list: Entry[]): boolean => {
    const index = list.findIndex((e) => same(e.action, action));
    if (index >= 0) {
      const entry = list[index];
      if (entry.radio) list.forEach((e, i) => { if (e.radio) e.checked = i === index; });
      else if (entry.checked !== undefined && (action.at(-1) === 'true' || action.at(-1) === 'false')) {
        entry.checked = action.at(-1) === 'true';
        entry.action = [...action.slice(0, -1), action.at(-1) === 'true' ? 'false' : 'true'];
      }
      return true;
    }
    return list.some((e) => walk(e.children));
  };
  if (!walk(value.entries) && value.moduleOptions) walk(value.moduleOptions);
}

function syncCollapse(enabled: boolean, value: Snapshot) {
  if (value.dashboard) value.dashboard.allowCollapse = enabled;
  for (const option of value.moduleOptions ?? []) {
    if (option.action?.[0] === 'set-collapse') { option.checked = enabled; option.action = ['set-collapse', enabled ? 'false' : 'true']; }
  }
}

function applySettings(settings: PresentationPreferences, value: Snapshot) {
  value.preferences = settings;
  value.appearance = settings.appearance;
  if (value.dashboard) {
    value.dashboard.allowCollapse = settings.allowCollapse;
    value.dashboard.expandedProviders = settings.expandedProviders;
    value.dashboard.showSummary = settings.visible.includes('summary');
    value.dashboard.providers = value.dashboard.providers.filter((p) => settings.visible.includes(p.id));
  }
  for (const option of value.moduleOptions ?? []) {
    const id = option.action?.[0] === 'set-module' ? option.action[1] : undefined;
    if (id) applyPreference(['set-module', id, settings.visible.includes(id) ? 'true' : 'false'], value);
  }
  syncCollapse(settings.allowCollapse, value);
  applyPreference(['set-appearance', settings.appearance], value);
  applyPreference(['set-theme', settings.theme ?? 'system'], value);
}

function applyPreference(action: string[], value: Snapshot) {
  const [kind] = action;
  if (kind === 'set-module' && action.length === 3) {
    for (const option of value.moduleOptions ?? []) {
      if (option.action?.[0] === 'set-module' && option.action[1] === action[1]) {
        const enabled = action[2] === 'true';
        option.checked = enabled;
        option.action = ['set-module', action[1], enabled ? 'false' : 'true'];
      }
    }
  }
  if (kind === 'set-collapse' && action.length === 2) {
    const enabled = action[1] === 'true';
    // Turning folding on starts with every card folded.
    if (enabled && value.dashboard) value.dashboard.expandedProviders = [];
    syncCollapse(enabled, value);
  }
  if (kind === 'set-expanded' && action.length === 3 && value.dashboard) {
    const expanded = new Set(value.dashboard.expandedProviders ?? []);
    if (action[2] === 'true') expanded.add(action[1]); else expanded.delete(action[1]);
    value.dashboard.expandedProviders = [...expanded].sort();
  }
  if ((kind === 'set-appearance' || kind === 'set-theme') && action.length === 2) {
    if (kind === 'set-appearance') value.appearance = action[1]; else value.theme = action[1];
    const update = (entries: Entry[]) => {
      for (const e of entries) {
        if (e.action?.[0] === kind) e.checked = e.action.at(-1) === action[1];
        update(e.children);
      }
    };
    if (value.moduleOptions) update(value.moduleOptions);
  }
}
