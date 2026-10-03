// Open record tabs, VS Code style: browsing reuses one "preview" tab until a
// tab is kept (double-click, or following a link); links navigate inside a
// tab with its own back history.

/** The "list" index of the NPC dialogs (rows are dialog indexes). */
export const DIALOGS = -1;

export interface Location {
  /** A list index, or DIALOGS. */
  list: number;
  /** Null until the list's records are loaded (then its first record). */
  row: number | null;
}

export interface Tab extends Location {
  id: number;
  /** Kept tabs stay open; the preview tab is replaced by the next record opened. */
  pinned: boolean;
  /** Locations to go back to, most recent last. */
  history: Location[];
}

export interface TabsState {
  tabs: Tab[];
  active: number | null;
}

export type TabAction =
  /** Open a record: reuse a tab already showing it, else the preview tab. */
  | { type: "open"; loc: Location; pin?: boolean; newTab?: boolean }
  /** Follow a link inside the active tab (keeps the tab, records history). */
  | { type: "navigate"; loc: Location }
  | { type: "back" }
  /** The active tab's list has loaded: settle a pending first row. */
  | { type: "settle"; row: number | null }
  | { type: "activate"; id: number }
  | { type: "pin"; id: number }
  | { type: "close"; id: number }
  | { type: "closeOthers"; id: number }
  | { type: "cycle"; delta: number }
  | { type: "activateIndex"; index: number }
  | { type: "reset"; state: TabsState }
  /** Rows of a list moved (a record inserted or removed at `at`). */
  | { type: "shiftRows"; list: number; at: number; delta: number; count: number };

export const EMPTY_TABS: TabsState = { tabs: [], active: null };

const MAX_HISTORY = 50;
let nextId = 1;

export function makeTab(loc: Location, pinned: boolean, history: Location[] = []): Tab {
  return { id: nextId++, list: loc.list, row: loc.row, pinned, history };
}

const sameLocation = (a: Location, b: Location) => a.list === b.list && a.row === b.row;

function insertAfterActive(state: TabsState, tab: Tab): TabsState {
  const at = state.tabs.findIndex((t) => t.id === state.active);
  const tabs = [...state.tabs];
  tabs.splice(at < 0 ? tabs.length : at + 1, 0, tab);
  return { tabs, active: tab.id };
}

const update = (state: TabsState, id: number, patch: Partial<Tab>): TabsState => ({
  ...state,
  tabs: state.tabs.map((t) => (t.id === id ? { ...t, ...patch } : t)),
});

export function tabsReducer(state: TabsState, action: TabAction): TabsState {
  const active = state.tabs.find((t) => t.id === state.active) ?? null;
  switch (action.type) {
    case "open": {
      const { loc, pin = false } = action;
      if (action.newTab) return insertAfterActive(state, makeTab(loc, true));
      // A tab already showing it (for a whole list: any tab of that list).
      const existing =
        loc.row === null
          ? (active?.list === loc.list ? active : state.tabs.find((t) => t.list === loc.list))
          : state.tabs.find((t) => sameLocation(t, loc));
      if (existing) {
        const next = { ...state, active: existing.id };
        return pin ? update(next, existing.id, { pinned: true }) : next;
      }
      const preview = active && !active.pinned ? active : state.tabs.find((t) => !t.pinned);
      if (preview) {
        return { ...update(state, preview.id, { list: loc.list, row: loc.row, pinned: pin, history: [] }), active: preview.id };
      }
      return insertAfterActive(state, makeTab(loc, pin));
    }
    case "navigate": {
      if (!active) return insertAfterActive(state, makeTab(action.loc, true));
      if (sameLocation(active, action.loc)) return state;
      const history = active.row === null ? active.history : [...active.history, { list: active.list, row: active.row }].slice(-MAX_HISTORY);
      return update(state, active.id, { list: action.loc.list, row: action.loc.row, pinned: true, history });
    }
    case "back": {
      const previous = active?.history.at(-1);
      if (!active || !previous) return state;
      return update(state, active.id, { ...previous, history: active.history.slice(0, -1) });
    }
    case "settle":
      return active && active.row === null ? update(state, active.id, { row: action.row }) : state;
    case "activate":
      return state.tabs.some((t) => t.id === action.id) ? { ...state, active: action.id } : state;
    case "pin":
      return update(state, action.id, { pinned: true });
    case "close": {
      const at = state.tabs.findIndex((t) => t.id === action.id);
      if (at < 0) return state;
      const tabs = state.tabs.filter((t) => t.id !== action.id);
      if (state.active !== action.id) return { ...state, tabs };
      const neighbour = tabs[at] ?? tabs[at - 1] ?? null;
      return { tabs, active: neighbour?.id ?? null };
    }
    case "closeOthers":
      return { tabs: state.tabs.filter((t) => t.id === action.id), active: action.id };
    case "cycle": {
      if (!state.tabs.length) return state;
      const at = Math.max(0, state.tabs.findIndex((t) => t.id === state.active));
      const n = state.tabs.length;
      return { ...state, active: state.tabs[(((at + action.delta) % n) + n) % n].id };
    }
    case "activateIndex": {
      const tab = action.index < 0 ? state.tabs.at(-1) : state.tabs[action.index];
      return tab ? { ...state, active: tab.id } : state;
    }
    case "reset":
      return action.state;
    case "shiftRows": {
      const { list, at, delta, count } = action;
      // A removed record's tab shows the record that took its place.
      const move = (row: number | null, removedGoes: boolean): number | null | undefined => {
        if (row === null || row < at) return row;
        if (delta > 0) return row + delta;
        if (row > at) return row + delta;
        return removedGoes ? undefined : count === 0 ? null : Math.min(at, count - 1);
      };
      return {
        ...state,
        tabs: state.tabs.map((t) => {
          if (t.list !== list) return t;
          const history = t.history
            .map((h) => (h.list === list ? { ...h, row: move(h.row, true) } : h))
            .filter((h): h is Location => h.row !== undefined);
          return { ...t, row: move(t.row, false) as number | null, history };
        }),
      };
    }
  }
}

// ---------------------------------------------------------------- persistence

const storageKey = (path: string) => `jdide.tabs:${path}`;

interface Saved {
  tabs: { list: number; row: number | null; pinned: boolean }[];
  active: number;
}

/** Tabs remembered for a file, checked against its lists and dialogs (null if none fit). */
export function loadTabs(path: string, counts: number[], dialogs: number): TabsState | null {
  try {
    const raw = localStorage.getItem(storageKey(path));
    if (!raw) return null;
    const saved = JSON.parse(raw) as Saved;
    const tabs = saved.tabs
      .filter((t) => {
        const n = t.list === DIALOGS ? dialogs : t.list >= 0 && t.list < counts.length ? counts[t.list] : -1;
        return n >= 0 && (t.row === null || t.row < n);
      })
      .map((t) => makeTab({ list: t.list, row: t.row }, t.pinned));
    if (!tabs.length) return null;
    return { tabs, active: (tabs[saved.active] ?? tabs[0]).id };
  } catch {
    return null;
  }
}

export function saveTabs(path: string, state: TabsState) {
  try {
    const saved: Saved = {
      tabs: state.tabs.map(({ list, row, pinned }) => ({ list, row, pinned })),
      active: Math.max(0, state.tabs.findIndex((t) => t.id === state.active)),
    };
    localStorage.setItem(storageKey(path), JSON.stringify(saved));
  } catch {
    // Remembering tabs is a convenience only.
  }
}
