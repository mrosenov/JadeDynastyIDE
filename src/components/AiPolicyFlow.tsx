import { useMemo, useState } from "react";
import { X } from "lucide-react";
import type { AiCondition, AiPolicyView } from "../elements/types";

interface Props {
  policy: AiPolicyView;
  onShowTrigger: (index: number) => void;
  onClose: () => void;
}

type Kind = "run" | "enable" | "disable" | "timer";
interface Edge { from: number; to: number; kind: Kind; label: string }

const ROW = 40, BOX_H = 30, BOX_W = 320, TOP = 16;
const KIND_LABELS: Record<Kind, string> = { run: "runs", enable: "enables", disable: "disables", timer: "timer (starts → tested by)" };

function timersTested(node: AiCondition | null | undefined, out: number[]) {
  if (!node) return;
  if (node.kind === 0) { const value = node.params.find((param) => param.key === "timer")?.value; if (typeof value === "number") out.push(value); }
  timersTested(node.left, out);
  timersTested(node.right, out);
}

/** A policy as an arc diagram: triggers in order, links between them on the right, timers on the left. */
export function AiPolicyFlow({ policy, onShowTrigger, onClose }: Props) {
  const [hover, setHover] = useState<number | null>(null);
  const { edges, missing } = useMemo(() => {
    const edges: Edge[] = [];
    const missing = new Map<number, string[]>();
    const byId = new Map<number, number>();
    policy.triggers.forEach((trigger, index) => { if (!byId.has(trigger.id)) byId.set(trigger.id, index); });
    const testers = new Map<number, number[]>();
    policy.triggers.forEach((trigger, index) => {
      const timers: number[] = [];
      timersTested(trigger.condition, timers);
      for (const timer of new Set(timers)) testers.set(timer, [...(testers.get(timer) ?? []), index]);
    });
    policy.triggers.forEach((trigger, index) => {
      for (const operation of trigger.operations) {
        const value = operation.params[0]?.value;
        if (typeof value !== "number") continue;
        const kind: Kind | null = operation.kind === 4 ? "run" : operation.kind === 6 ? "enable" : operation.kind === 5 ? "disable" : operation.kind === 7 ? "timer" : null;
        if (!kind) continue;
        if (kind === "timer") {
          const targets = testers.get(value) ?? [];
          if (!targets.length) missing.set(index, [...(missing.get(index) ?? []), `timer ${value} (nothing tests it)`]);
          for (const to of targets) if (!edges.some((edge) => edge.kind === "timer" && edge.from === index && edge.to === to && edge.label === String(value))) edges.push({ from: index, to, kind, label: String(value) });
        } else {
          const to = byId.get(value);
          if (to === undefined) missing.set(index, [...(missing.get(index) ?? []), `${KIND_LABELS[kind]} ${value} (missing)`]);
          else if (!edges.some((edge) => edge.kind === kind && edge.from === index && edge.to === to)) edges.push({ from: index, to, kind, label: "" });
        }
      }
    });
    return { edges, missing };
  }, [policy]);

  const reach = (edge: Edge) => 18 + Math.min(240, Math.abs(edge.to - edge.from) * 14 + (edge.from === edge.to ? 10 : 0));
  const right = Math.max(40, ...edges.filter((edge) => edge.kind !== "timer").map(reach)) + 16;
  const left = Math.max(40, ...edges.filter((edge) => edge.kind === "timer").map(reach)) + 16;
  const height = TOP * 2 + policy.triggers.length * ROW;
  const width = left + BOX_W + right;
  const y = (index: number) => TOP + index * ROW + BOX_H / 2;
  const related = (edge: Edge) => hover === null || edge.from === hover || edge.to === hover;
  const path = (edge: Edge) => {
    const r = reach(edge);
    const offset = edge.from === edge.to ? 6 : 0;
    if (edge.kind === "timer") {
      const x = left;
      return `M ${x} ${y(edge.from) - offset} C ${x - r} ${y(edge.from) - offset}, ${x - r} ${y(edge.to) + offset}, ${x} ${y(edge.to) + offset}`;
    }
    const x = left + BOX_W;
    return `M ${x} ${y(edge.from) - offset} C ${x + r} ${y(edge.from) - offset}, ${x + r} ${y(edge.to) + offset}, ${x} ${y(edge.to) + offset}`;
  };

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal ai-flow-dialog" role="dialog" aria-label="Policy flow" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head">
        <h3>Flow of policy {policy.id}</h3>
        <span className="ai-flow-legend">{(Object.keys(KIND_LABELS) as Kind[]).map((kind) => <span key={kind}><svg width="22" height="8"><line x1="0" y1="4" x2="22" y2="4" className={"ai-flow-edge " + kind} /></svg>{KIND_LABELS[kind]}</span>)}</span>
        <span className="spacer" />
        <button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button>
      </header>
      <div className="ai-flow-scroll">
        {!policy.triggers.length ? <div className="empty-note">No triggers.</div> : <svg width={width} height={height} className="ai-flow">
          <defs>{(Object.keys(KIND_LABELS) as Kind[]).map((kind) => <marker key={kind} id={`ai-arrow-${kind}`} viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L8,4 L0,8 z" className={"ai-flow-head " + kind} /></marker>)}</defs>
          {edges.map((edge, index) => <g key={index} className={related(edge) ? "" : "ai-flow-dim"}>
            <path d={path(edge)} className={"ai-flow-edge " + edge.kind} markerEnd={`url(#ai-arrow-${edge.kind})`}><title>{`${policy.triggers[edge.from].id} ${KIND_LABELS[edge.kind]}${edge.label ? ` ${edge.label}` : ""} → ${policy.triggers[edge.to].id}`}</title></path>
            {edge.label && <text x={left - reach(edge) * 0.75 - 4} y={(y(edge.from) + y(edge.to)) / 2 + 4} className="ai-flow-label" textAnchor="end">{edge.label}</text>}
          </g>)}
          {policy.triggers.map((trigger, index) => {
            const problems = missing.get(index);
            return <g key={index} className={"ai-flow-node" + (trigger.run ? " sub" : "") + (hover === index ? " hover" : "")} transform={`translate(${left}, ${TOP + index * ROW})`} onMouseEnter={() => setHover(index)} onMouseLeave={() => setHover(null)} onClick={() => onShowTrigger(index)}>
              <title>{`${trigger.expression}${problems ? `\n${problems.join("\n")}` : ""}`}</title>
              <rect width={BOX_W} height={BOX_H} rx={6} />
              <text x={8} y={19} className="ai-flow-id">{trigger.id}</text>
              <text x={48} y={19} className="ai-flow-name">{(trigger.name || trigger.expression).slice(0, 30)}</text>
              <text x={BOX_W - 8} y={19} className="ai-flow-cat" textAnchor="end">{trigger.run ? "Sub" : trigger.category}{!trigger.run && !trigger.active && !["Combat start", "Death", "Path end", "Birth", "Skill hit"].includes(trigger.category) ? " · off" : ""}</text>
              {problems && <circle cx={BOX_W - 2} cy={2} r={5} className="ai-flow-missing" />}
            </g>;
          })}
        </svg>}
      </div>
      <footer className="modal-foot muted small">Triggers in the order the server tests them. Right: "Run a trigger", "Enable a trigger" and "Disable a trigger"; left: a trigger starting a timer and the triggers testing it. A red dot marks a link to a missing trigger or a timer nothing tests. Hover a trigger to see only its links; click it to open it.</footer>
    </div>
  </div>;
}
