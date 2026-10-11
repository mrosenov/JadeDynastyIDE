// A policy simulator following the server (gs/aitrigger.h `policy`, `trigger::TestTrigger`, aipolicy.cpp
// `ai_policy::OnHeartbeat`/`EnableCombat`/`RollBack`/`OnDeath`). Only top-level triggers are in the event lists;
// "Run a trigger" tests a copy of the sub-trigger, whose own enabled flag is never read.
import type { AiCondition, AiOperation, AiPolicyView, AiTrigger } from "./types";

export type RandomMode = "roll" | "always" | "never";

export interface SimSettings {
  /** HP in percent. */
  hp: number;
  /** Entries in the aggro list (players fighting it). */
  players: number;
  /** Metres from where combat started. */
  distance: number;
  random: RandomMode;
  /** Ranking levels by place (ranking level conditions). */
  ranks: Record<string, number>;
}

export interface SimTimer {
  id: number;
  timeout: number;
  period: number;
  /** Times left (0 = forever, -1 = finished). */
  times: number;
  inBattle: boolean;
}

export type LogLine =
  | { kind: "event"; time: number; text: string }
  | { kind: "fire"; time: number; depth: number; trigger: AiTrigger; note?: string }
  | { kind: "op"; time: number; depth: number; operation: AiOperation; note?: string }
  | { kind: "info"; time: number; depth: number; text: string; warn?: boolean };

export interface SimState {
  time: number;
  inCombat: boolean;
  dead: boolean;
  /** Enabled flags of the top-level triggers, by position in the policy. */
  enabled: boolean[];
  timers: SimTimer[];
  timerFlag: number;
  globals: Record<string, number>;
  log: LogLine[];
}

const START_COMBAT = "Combat start", HEARTBEAT = "Heartbeat", TIMER = "Timer";
/** Event lists whose triggers are enabled at start whatever their flag says (`AddTrigger` → `SetDefaultEnable(true)`). */
const FORCED = new Set([START_COMBAT, "Death", "Path end", "Birth", "Skill hit"]);
const MAX_LOG = 4000;
const MAX_DEPTH = 32;

export function defaultEnabled(trigger: AiTrigger) {
  return FORCED.has(trigger.category) || trigger.active;
}

export function initialState(policy: AiPolicyView, globals: Record<string, number> = {}): SimState {
  return { time: 0, inCombat: false, dead: false, enabled: policy.triggers.map(defaultEnabled), timers: [], timerFlag: 0, globals: { ...globals }, log: [] };
}

const num = (operation: AiOperation | AiCondition, key: string) => {
  const value = operation.params.find((param) => param.key === key)?.value;
  return typeof value === "number" ? value : typeof value === "boolean" ? Number(value) : 0;
};

/** Whether the server disables the trigger after it fires (`IsAutoDisable`). */
function autoDisable(node: AiCondition | null | undefined): boolean {
  if (!node) return false;
  switch (node.kind) {
    case 7: return autoDisable(node.left) && autoDisable(node.right);
    case 6: return autoDisable(node.left) || autoDisable(node.right);
    case 5: return autoDisable(node.right);
    case 1: case 10: case 11: case 23: return true;
    default: return false;
  }
}

/** One simulation run over a copy of the state; `step` functions mutate it. */
export class Simulator {
  readonly state: SimState;
  constructor(private policy: AiPolicyView, state: SimState, private settings: SimSettings, private newer: boolean) {
    this.state = { ...state, enabled: [...state.enabled], timers: state.timers.map((timer) => ({ ...timer })), globals: { ...state.globals }, log: [...state.log] };
  }

  private log(line: LogLine) {
    this.state.log.push(line);
    if (this.state.log.length > MAX_LOG) this.state.log.splice(0, this.state.log.length - MAX_LOG);
  }
  private info(text: string, depth = 0, warn = false) {
    this.log({ kind: "info", time: this.state.time, depth, text, warn });
  }

  private top(category: string) {
    return this.policy.triggers.map((trigger, index) => ({ trigger, index })).filter(({ trigger }) => !trigger.run && trigger.category === category);
  }

  private random(chance: number) {
    if (this.settings.random === "always") return chance > 0;
    if (this.settings.random === "never") return false;
    return Math.random() < chance;
  }

  private value(node: AiCondition | null | undefined): number {
    if (!node) return 0;
    const left = () => this.value(node.left), right = () => this.value(node.right);
    switch (node.kind) {
      case 12: return (left() + right()) | 0;
      case 13: return (left() - right()) | 0;
      case 14: return Math.imul(left(), right());
      case 15: {
        const divisor = right();
        if (!divisor) { this.info("Division by zero in a condition (the server's result is undefined); using 0", 1, true); return 0; }
        return (left() / divisor) | 0;
      }
      case 19: return this.state.globals[String(num(node, "variable"))] ?? 0;
      case 20: return num(node, "value");
      case 21: return this.settings.ranks[String(num(node, "place"))] ?? 0;
      default: return 0;
    }
  }

  private check(node: AiCondition | null | undefined, param: number): boolean {
    if (!node) return false;
    switch (node.kind) {
      case 0: return this.checkTimer(num(node, "timer"));
      case 1: return this.settings.hp / 100 < num(node, "ratio");
      case 3: return this.random(num(node, "chance"));
      case 5: return !this.check(node.right, param);
      case 6: return this.check(node.left, param) || this.check(node.right, param);
      case 7: return this.check(node.left, param) && this.check(node.right, param);
      case 9: return param === num(node, "path");
      case 10: return this.settings.players > num(node, "players");
      case 11: return this.settings.distance > num(node, "distance");
      case 16: return this.value(node.left) > this.value(node.right);
      case 17: return this.value(node.left) < this.value(node.right);
      case 18: return this.value(node.left) === this.value(node.right);
      case 23: return param === num(node, "skill");
      case 2: case 4: case 8: case 22: case 24: case 25: return true;
      default: return false;
    }
  }

  private checkTimer(id: number) {
    if (!this.state.timerFlag) return false;
    const timer = this.state.timers.find((entry) => entry.id === id);
    return !!timer && timer.timeout === 0;
  }

  /** `trigger::TestTrigger`: false when an operation stopped the list (the event list stops too). */
  private test(trigger: AiTrigger, index: number | null, param: number, depth: number): boolean {
    if (!this.check(trigger.condition, param)) {
      if (index === null) this.info(`Sub-trigger ${trigger.id}: its condition does not hold (${trigger.expression})`, depth);
      return true;
    }
    const once = autoDisable(trigger.condition);
    this.log({ kind: "fire", time: this.state.time, depth, trigger, note: once && index !== null ? "fires once: disabled until it resets" : undefined });
    let result = true;
    const operations = trigger.operations;
    if (trigger.selectOne) {
      if (operations.length) {
        const picked = Math.floor(Math.random() * operations.length);
        result = this.run(operations[picked], trigger, depth + 1, `picked ${picked + 1} of ${operations.length} at random`);
      }
    } else {
      for (const operation of operations) {
        if (!this.run(operation, trigger, depth + 1)) { result = false; break; }
      }
    }
    if (once && index !== null) this.state.enabled[index] = false;
    return result;
  }

  private run(operation: AiOperation, trigger: AiTrigger, depth: number, note?: string): boolean {
    this.log({ kind: "op", time: this.state.time, depth, operation, note });
    const id = num(operation, operation.params[0]?.key ?? "");
    const hasTarget = operation.target === 7 || this.settings.players > 0;
    switch (operation.kind) {
      case 4: {
        const sub = this.policy.triggers.find((entry) => entry.id === id);
        if (!sub) {
          this.info(this.newer ? `No trigger ${id}: nothing happens` : `No trigger ${id}: the 2013 server crashes loading this policy`, depth, true);
          return true;
        }
        if (depth > MAX_DEPTH) { this.info("Runs too deep (a loop): the server never finishes loading this policy", depth, true); return false; }
        const result = this.test(sub, null, 0, depth + 1);
        if (!result) this.info("The sub-trigger stopped its operations: this list stops too", depth, true);
        return result;
      }
      case 5: case 6: {
        const enable = operation.kind === 6;
        let found = false;
        this.policy.triggers.forEach((entry, index) => { if (entry.id === id && !entry.run) { this.state.enabled[index] = enable; found = true; } });
        if (!found) this.info(`No top-level trigger ${id} to ${enable ? "enable" : "disable"} (sub-triggers are copies; their flag is never read)`, depth, true);
        return true;
      }
      case 7: {
        const period = num(operation, "period"), times = num(operation, "times");
        const existing = this.state.timers.find((timer) => timer.id === id);
        if (existing) Object.assign(existing, { timeout: period, period, times });
        else this.state.timers.push({ id, timeout: period, period, times, inBattle: trigger.battleOnly });
        return true;
      }
      case 8: {
        const at = this.state.timers.findIndex((timer) => timer.id === id);
        if (at >= 0) this.state.timers.splice(at, 1);
        return true;
      }
      case 13: return false;
      case 22: this.state.globals[String(num(operation, "variable"))] = num(operation, "value"); return true;
      case 23: { const key = String(num(operation, "variable")); this.state.globals[key] = ((this.state.globals[key] ?? 0) + num(operation, "value")) | 0; return true; }
      case 24: this.state.globals[String(num(operation, "to"))] = this.state.globals[String(num(operation, "from"))] ?? 0; return true;
      case 27:
        if (!hasTarget) { this.info("No target (empty aggro list): the operation fails and the list stops", depth, true); return false; }
        return true;
      default: return true;
    }
  }

  /** `policy::CheckTriggers`: enabled triggers in order, stopping when one returns false. */
  private checkList(category: string, param = 0, ignoreEnabled = false, noBreak = false, peaceOnly = false) {
    for (const { trigger, index } of this.top(category)) {
      if (peaceOnly && trigger.battleOnly) continue;
      if (!ignoreEnabled && !this.state.enabled[index]) continue;
      if (!this.test(trigger, index, param, 1) && !noBreak) {
        this.info("An operation stopped: later triggers of this event are skipped this time", 1, true);
        return;
      }
    }
  }

  private refreshTimers() {
    this.state.timerFlag = 0;
    for (const timer of this.state.timers) {
      if (!timer.timeout) timer.timeout = timer.period;
      timer.timeout--;
      if (timer.timeout === 0) {
        this.state.timerFlag |= 1;
        this.info(`Timer ${timer.id} fires${timer.times > 0 ? timer.times === 1 ? " (last time)" : ` (${timer.times - 1} more)` : ""}`);
        if (timer.times > 0) {
          timer.times--;
          if (!timer.times) { timer.times = -1; this.state.timerFlag |= 2; }
        }
      }
    }
  }

  private timersAndRemoval() {
    this.refreshTimers();
    if (this.state.timerFlag & 1) this.checkList(TIMER, 0, false, true);
    if (this.state.timerFlag & 2) {
      this.state.timerFlag = 0;
      this.state.timers = this.state.timers.filter((timer) => timer.times >= 0);
    }
  }

  /** `policy::Reset` (leaving combat): battle triggers back to their start, battle timers removed. */
  private resetBattle() {
    this.policy.triggers.forEach((trigger, index) => { if (!trigger.run && trigger.battleOnly) this.state.enabled[index] = defaultEnabled(trigger); });
    this.state.timerFlag = 0;
    this.state.timers = this.state.timers.filter((timer) => !timer.inBattle);
  }

  private event(text: string) {
    this.log({ kind: "event", time: this.state.time, text });
  }

  heartbeat() {
    if (this.state.dead) return;
    this.state.time++;
    if (this.state.inCombat) {
      this.timersAndRemoval();
      this.checkList(HEARTBEAT);
      // OnPeaceHeartbeatInCombat: triggers that are not combat-only are tested a second time.
      this.checkList(HEARTBEAT, 0, false, false, true);
    } else {
      this.timersAndRemoval();
      this.checkList(HEARTBEAT, 0, false, false, true);
    }
  }

  spawn() {
    this.event("Spawns");
    this.checkList("Birth");
  }

  startCombat() {
    if (this.state.inCombat || this.state.dead) return;
    this.event("Combat starts");
    this.state.inCombat = true;
    // StartCombat → CheckTriggersNoTest: these run even when disabled.
    this.checkList(START_COMBAT, 0, true);
  }

  leaveCombat() {
    if (!this.state.inCombat || this.state.dead) return;
    this.event("Leaves combat (aggro list empty)");
    this.resetBattle();
    this.state.inCombat = false;
    this.info("Combat-only triggers are back to their start state; combat timers are removed");
    this.checkList("Leave combat");
  }

  kill() {
    if (this.state.dead) return;
    this.event("Kills its target");
    this.checkList("Kill");
  }

  skillHit(skill: number) {
    if (this.state.dead) return;
    this.event(`Hit by skill ${skill}`);
    this.checkList("Skill hit", skill);
  }

  pathEnd(path: number) {
    if (this.state.dead) return;
    this.event(`Reaches the end of path ${path}`);
    this.checkList("Path end", path);
  }

  die() {
    if (this.state.dead) return;
    this.event("Dies");
    if (this.state.inCombat) this.resetBattle();
    this.state.inCombat = false;
    this.checkList("Death");
    // ResetAll: every trigger back to its start, every timer removed.
    this.state.enabled = this.policy.triggers.map(defaultEnabled);
    this.state.timers = [];
    this.state.timerFlag = 0;
    this.state.dead = true;
    this.info("Every trigger is back to its start state and every timer is removed");
  }
}
