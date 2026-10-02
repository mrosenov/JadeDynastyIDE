// NPC dialogs: option targets and the predefined functions.

import type { TalkOption, TalkWindow } from "./types";

export const FUNCTION_BIT = 0x8000_0000;
/** `parent` of the root window. */
export const NO_PARENT = 0xffff_ffff;

/**
 * SERVICE_TYPE (ExpTypes.h): option IDs from 0x80000000 on. Dialogs in
 * elements.data mostly use Back and Exit; NPC services use the rest.
 */
const FUNCTIONS = [
  "NPC talk",
  "Sell",
  "Buy",
  "Repair",
  "Install",
  "Uninstall",
  "Give task",
  "Complete task",
  "Give task item",
  "Teach skill",
  "Heal",
  "Teleport",
  "Transport",
  "Proxy sale",
  "Storage",
  "Craft",
  "Decompose",
  "Back",
  "Exit",
  "Storage password",
  "Identify",
  "Give up task",
  "Build war tower",
  "Reset attributes",
  "Bind equipment",
  "Destroy equipment",
  "Undo destroy equipment",
  "Buy war archer",
  "Item trade",
  "Equip soul meld",
  "Consignment",
  "Instance",
];

export const TALK_RETURN = FUNCTION_BIT + 17;
export const TALK_EXIT = FUNCTION_BIT + 18;
/** Functions whose param is a task ID. */
const TASK_FUNCTIONS = new Set([6, 7, 8, 21].map((n) => FUNCTION_BIT + n));

export const isFunction = (o: TalkOption) => o.id >= FUNCTION_BIT;

export function functionName(id: number): string {
  return FUNCTIONS[id - FUNCTION_BIT] ?? `Function 0x${id.toString(16)}`;
}

/** What an option's param means, when it means something. */
export function paramNote(o: TalkOption): string | null {
  if (TASK_FUNCTIONS.has(o.id) && o.param) return `task ${o.param}`;
  return null;
}

export const windowsById = (windows: TalkWindow[]) => new Map(windows.map((w) => [w.id, w]));

/** The root window: parent -1, else the first one. */
export const rootWindow = (windows: TalkWindow[]) => windows.find((w) => w.parent === NO_PARENT) ?? windows[0] ?? null;

/** Window IDs from the root down to `id`, following parents. */
export function pathTo(windows: TalkWindow[], id: number): number[] {
  const byId = windowsById(windows);
  const path: number[] = [];
  let w = byId.get(id);
  while (w && !path.includes(w.id)) {
    path.unshift(w.id);
    w = w.parent === NO_PARENT ? undefined : byId.get(w.parent);
  }
  return path;
}
