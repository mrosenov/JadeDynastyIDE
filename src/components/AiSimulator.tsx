import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Play, RotateCcw, X } from "lucide-react";
import { initialState, Simulator, type LogLine, type RandomMode, type SimSettings, type SimState } from "../elements/aiSim";
import type { AiCondition, AiOperation, AiPolicyView } from "../elements/types";

interface Props {
  policy: AiPolicyView;
  /** The file is for the newer server (missing sub-triggers are no-ops there). */
  newer: boolean;
  /** An operation with its values and names, as the editor shows it. */
  describe: (operation: AiOperation) => ReactNode;
  onShowTrigger: (index: number) => void;
  onClose: () => void;
}

const SETTINGS: SimSettings = { hp: 100, players: 1, distance: 0, random: "roll", ranks: {} };

/** Values a policy's conditions and operations name: global variables, ranking places, skills hit by, paths. */
function referenced(policy: AiPolicyView) {
  const globals = new Set<number>(), ranks = new Set<number>(), skills = new Set<number>(), paths = new Set<number>();
  const num = (node: { params: { key: string; value: unknown }[] }, key: string) => { const value = node.params.find((param) => param.key === key)?.value; return typeof value === "number" ? value : null; };
  const walk = (node: AiCondition | null | undefined) => {
    if (!node) return;
    if (node.kind === 19) { const value = num(node, "variable"); if (value !== null) globals.add(value); }
    if (node.kind === 21) { const value = num(node, "place"); if (value !== null) ranks.add(value); }
    if (node.kind === 23) { const value = num(node, "skill"); if (value !== null) skills.add(value); }
    if (node.kind === 9) { const value = num(node, "path"); if (value !== null) paths.add(value); }
    walk(node.left);
    walk(node.right);
  };
  for (const trigger of policy.triggers) {
    walk(trigger.condition);
    for (const operation of trigger.operations) {
      for (const key of operation.kind === 24 ? ["from", "to"] : operation.kind === 22 || operation.kind === 23 ? ["variable"] : []) {
        const value = num(operation, key);
        if (value !== null) globals.add(value);
      }
    }
  }
  const sorted = (set: Set<number>) => [...set].sort((a, b) => a - b);
  return { globals: sorted(globals), ranks: sorted(ranks), skills: sorted(skills), paths: sorted(paths) };
}

function NumberBox({ value, onChange, min, max, step, width = 70 }: { value: number; onChange: (value: number) => void; min?: number; max?: number; step?: number; width?: number }) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const number = Number(draft);
    if (draft.trim() === "" || !Number.isFinite(number)) { setDraft(String(value)); return; }
    const clamped = Math.min(max ?? Infinity, Math.max(min ?? -Infinity, number));
    setDraft(String(clamped));
    if (clamped !== value) onChange(clamped);
  };
  return <input className="dyn-input mono" style={{ width }} type="number" min={min} max={max} step={step} value={draft} onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Enter") commit(); }} />;
}

/** Runs a policy the way the server does: events, heartbeats and timers, with a log of what fires. */
export function AiSimulator({ policy, newer, describe, onShowTrigger, onClose }: Props) {
  const refs = useMemo(() => referenced(policy), [policy]);
  const [settings, setSettings] = useState<SimSettings>(SETTINGS);
  const [state, setState] = useState<SimState>(() => initialState(policy));
  const [skill, setSkill] = useState(() => refs.skills[0] ?? 0);
  const [path, setPath] = useState(() => refs.paths[0] ?? 0);
  const [seconds, setSeconds] = useState(10);
  const logRef = useRef<HTMLDivElement>(null);

  // An edited policy starts over.
  useEffect(() => setState(initialState(policy)), [policy]);
  useEffect(() => { const box = logRef.current; if (box) box.scrollTop = box.scrollHeight; }, [state.log.length]);

  const step = (action: (sim: Simulator) => void) => setState((current) => {
    const sim = new Simulator(policy, current, settings, newer);
    action(sim);
    return sim.state;
  });
  const restart = () => setState(initialState(policy));
  const set = <K extends keyof SimSettings>(key: K, value: SimSettings[K]) => setSettings((current) => ({ ...current, [key]: value }));

  const topLevel = policy.triggers.map((trigger, index) => ({ trigger, index })).filter(({ trigger }) => !trigger.run);
  const lineText = (line: LogLine): ReactNode => {
    switch (line.kind) {
      case "event": return <b>{line.text}</b>;
      case "fire": return <><button className="link mono" onClick={() => onShowTrigger(policy.triggers.indexOf(line.trigger))}>Trigger {line.trigger.id}</button> {line.trigger.name && <span>{line.trigger.name}</span>} <span className="muted">fires ({line.trigger.expression})</span>{line.note && <span className="muted small"> · {line.note}</span>}</>;
      case "op": return <>{describe(line.operation)}{line.note && <span className="muted small"> · {line.note}</span>}</>;
      case "info": return <span className={line.warn ? "ai-warn" : "muted"}>{line.text}</span>;
    }
  };

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal ai-sim-dialog" role="dialog" aria-label="Simulate the policy" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head">
        <h3>Simulate policy {policy.id}</h3>
        <span className="muted small">{state.dead ? "dead" : state.inCombat ? "in combat" : "out of combat"} · {state.time} s</span>
        <span className="spacer" />
        <button className="btn small" onClick={restart}><RotateCcw size={13} /> Restart</button>
        <button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button>
      </header>
      <div className="ai-sim-body">
        <section className="ai-sim-controls">
          <h4>Events</h4>
          <div className="ai-sim-buttons">
            <button className="btn small" onClick={() => step((sim) => sim.spawn())} disabled={state.dead}>Spawn</button>
            <button className="btn small" onClick={() => step((sim) => sim.startCombat())} disabled={state.dead || state.inCombat}>Start combat</button>
            <button className="btn small" onClick={() => step((sim) => sim.kill())} disabled={state.dead}>Kill its target</button>
            <button className="btn small" onClick={() => step((sim) => sim.leaveCombat())} disabled={state.dead || !state.inCombat}>Leave combat</button>
            <button className="btn small danger" onClick={() => step((sim) => sim.die())} disabled={state.dead}>Die</button>
          </div>
          <div className="ai-sim-row">
            <button className="btn small" onClick={() => step((sim) => sim.skillHit(skill))} disabled={state.dead}>Hit by skill</button>
            {refs.skills.length > 1 ? <select className="task-form-select" value={skill} onChange={(event) => setSkill(Number(event.target.value))}>{refs.skills.map((id) => <option key={id} value={id}>{id}</option>)}</select> : <NumberBox value={skill} min={0} onChange={setSkill} width={90} />}
          </div>
          <div className="ai-sim-row">
            <button className="btn small" onClick={() => step((sim) => sim.pathEnd(path))} disabled={state.dead}>Path end</button>
            {refs.paths.length > 1 ? <select className="task-form-select" value={path} onChange={(event) => setPath(Number(event.target.value))}>{refs.paths.map((id) => <option key={id} value={id}>{id}</option>)}</select> : <NumberBox value={path} min={0} onChange={setPath} width={90} />}
          </div>
          <div className="ai-sim-row">
            <button className="btn small primary" onClick={() => step((sim) => sim.heartbeat())} disabled={state.dead} title="One heartbeat: timers count down, then timer and heartbeat triggers are tested"><Play size={12} /> 1 s</button>
            <button className="btn small" onClick={() => step((sim) => { for (let i = 0; i < seconds && !sim.state.dead; i++) sim.heartbeat(); })} disabled={state.dead}>Run</button>
            <NumberBox value={seconds} min={1} max={3600} onChange={setSeconds} width={60} /><span className="muted small">s</span>
          </div>
          <h4>The monster</h4>
          <label className="ai-sim-field"><span>HP</span><input type="range" min={0} max={100} value={settings.hp} onChange={(event) => set("hp", Number(event.target.value))} /><NumberBox value={settings.hp} min={0} max={100} onChange={(value) => set("hp", value)} width={56} />%</label>
          <label className="ai-sim-field"><span>Players in aggro</span><NumberBox value={settings.players} min={0} max={999} onChange={(value) => set("players", value)} /></label>
          <label className="ai-sim-field"><span>Distance from the combat start</span><NumberBox value={settings.distance} min={0} onChange={(value) => set("distance", value)} />m</label>
          <label className="ai-sim-field"><span>Random conditions</span>
            <select className="task-form-select" value={settings.random} onChange={(event) => set("random", event.target.value as RandomMode)}>
              <option value="roll">Roll the chance</option>
              <option value="always">Always hold</option>
              <option value="never">Never hold</option>
            </select>
          </label>
          {refs.globals.length > 0 && <>
            <h4>Global variables</h4>
            <p className="muted small">Shared by the whole server; operations change them as the simulation runs.</p>
            {refs.globals.map((key) => <label key={key} className="ai-sim-field"><span className="mono">global[{key}]</span><NumberBox value={state.globals[String(key)] ?? 0} onChange={(value) => setState((current) => ({ ...current, globals: { ...current.globals, [String(key)]: value } }))} /></label>)}
          </>}
          {refs.ranks.length > 0 && <>
            <h4>Ranking levels</h4>
            {refs.ranks.map((place) => <label key={place} className="ai-sim-field"><span>Place {place}</span><NumberBox value={settings.ranks[String(place)] ?? 0} onChange={(value) => set("ranks", { ...settings.ranks, [String(place)]: value })} /></label>)}
          </>}
        </section>
        <section className="ai-sim-state">
          <h4>Triggers</h4>
          <div className="ai-sim-triggers">
            {topLevel.map(({ trigger, index }) => <button key={index} className={"ai-sim-trigger" + (state.enabled[index] ? "" : " off")} onClick={() => onShowTrigger(index)} title={trigger.expression}>
              <span className={"ai-sim-dot" + (state.enabled[index] ? " on" : "")} />
              <span className="mono muted">{trigger.id}</span>
              <span className="truncate">{trigger.name || trigger.expression}</span>
              <span className="tag ai-cat">{trigger.category}{trigger.battleOnly ? " · combat" : ""}</span>
            </button>)}
            {policy.triggers.length > topLevel.length && <p className="muted small">{policy.triggers.length - topLevel.length} sub-trigger{policy.triggers.length - topLevel.length === 1 ? "" : "s"} (run by others).</p>}
          </div>
          <h4>Timers</h4>
          {state.timers.length ? <table className="ai-sim-timers"><thead><tr><th>Timer</th><th>Next in</th><th>Every</th><th>Left</th></tr></thead><tbody>
            {state.timers.map((timer) => <tr key={timer.id}><td className="mono">{timer.id}{timer.inBattle && <span className="muted small"> combat</span>}</td><td>{timer.timeout || timer.period} s</td><td>{timer.period} s</td><td>{timer.times === 0 ? "forever" : timer.times < 0 ? "done" : timer.times}</td></tr>)}
          </tbody></table> : <p className="muted small">No timer running.</p>}
        </section>
        <section className="ai-sim-log" ref={logRef}>
          {!state.log.length ? <div className="empty-note">Start with <b>Spawn</b> or <b>Start combat</b>, then let time pass with <b>1 s</b> or <b>Run</b>. Set HP, players and distance on the left; heartbeat triggers test them every second.</div>
            : state.log.map((line, index) => <div key={index} className={"ai-sim-line " + line.kind} style={{ paddingLeft: 8 + ("depth" in line ? line.depth : 0) * 16 }}>
              <span className="ai-sim-time mono muted">{line.time}s</span>{lineText(line)}
            </div>)}
        </section>
      </div>
      <footer className="modal-foot muted small">Follows the server's rules: event lists hold only top-level triggers; combat start runs its triggers even when disabled; an operation that stops (Skip the remaining operations, Change aggro without a target) also skips the later triggers of that event; leaving combat resets combat-only triggers and timers; dying resets everything. Skills, summons and talks are only listed.</footer>
    </div>
  </div>;
}
