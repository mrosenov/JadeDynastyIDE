import { useMemo, useState } from "react";
import { Check, Loader2, Plus, Search, Trash2, X } from "lucide-react";
import { monsterPolicySlots, pickEssence, setMonsterPolicies } from "../elements/api";
import { count } from "../elements/format";
import type { EditState, PolicySlot } from "../elements/types";
import { ValuePicker } from "./ValuePicker";

interface Props {
  policyId: number;
  /** Monsters using it now: [ID, name]. */
  users: [number, string][];
  onApplied: (state: EditState, count: number) => void;
  onClose: () => void;
}

const slotKey = (slot: PolicySlot) => `${slot.list}:${slot.row}:${slot.off}`;

/** Sets this policy on monsters of the open elements.data (their common_strategy, or fields with the aipolicy role). */
export function AiUseForMonsters({ policyId, users, onApplied, onClose }: Props) {
  const [rows, setRows] = useState<PolicySlot[]>([]);
  const [ids, setIds] = useState("");
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const pending = useMemo(() => rows.filter((row) => row.policy !== policyId), [policyId, rows]);

  const add = async (wanted: number[]) => {
    if (!wanted.length) return;
    setError(null);
    setNote(null);
    try {
      const slots = await monsterPolicySlots(wanted);
      const found = new Set(slots.map((slot) => slot.id));
      const missing = wanted.filter((id) => !found.has(id));
      setRows((current) => {
        const keys = new Set(current.map(slotKey));
        return [...current, ...slots.filter((slot) => !keys.has(slotKey(slot)))];
      });
      if (missing.length) setNote(`Not added (not in elements.data, or no AI policy field): ${missing.slice(0, 12).join(", ")}${missing.length > 12 ? ", …" : ""}`);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    }
  };
  const addTyped = () => {
    const wanted = [...new Set(ids.split(/[^0-9]+/).filter(Boolean).map(Number).filter((id) => id > 0))];
    void add(wanted).then(() => setIds(""));
  };
  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      const state = await setMonsterPolicies(pending.map((row) => [row.list, row.row, row.off, row.policy]), policyId);
      onApplied(state, pending.length);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };

  return <><div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal npcgen-nearby-dialog ai-use-dialog" role="dialog" aria-label="Use for monsters" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Use policy {policyId} for monsters</h3><span className="muted small">Sets their <span className="mono">common_strategy</span> in the open elements.data</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="npcgen-nearby-bar">
        <button className="btn small" onClick={() => setPicking(true)}><Search size={13} /> Choose a monster…</button>
        <input className="dyn-input" value={ids} placeholder="Or monster IDs: 1234, 1235 …" onChange={(event) => setIds(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") addTyped(); }} />
        <button className="btn small" onClick={addTyped} disabled={!ids.trim()}><Plus size={13} /> Add</button>
      </div>
      {error && <div className="path-data-message error">{error}</div>}
      {note && <div className="path-data-message">{note}</div>}
      <div className="npcgen-nearby-table">
        {!rows.length ? <div className="empty-note">Add the monsters that should use policy {policyId}.{users.length ? <><br />Used now by {users.slice(0, 8).map(([id, name]) => `${name || "?"} (${id})`).join(", ")}{users.length > 8 ? ` and ${count(users.length - 8)} more` : ""}.</> : null}</div>
          : <table className="dyn-table">
            <thead><tr><th>Monster</th><th>Field</th><th>Policy now</th><th /></tr></thead>
            <tbody>{rows.map((row) => <tr key={slotKey(row)}>
              <td><span className="mono">{row.id}</span> {row.name}</td>
              <td className="mono small">{row.field}</td>
              <td>{row.policy === policyId ? <span className="muted">already {policyId}</span> : row.policy ? <span className="mono">{row.policy}</span> : <span className="muted">none</span>}</td>
              <td><button className="icon-btn small danger" title="Remove from the list" onClick={() => setRows((current) => current.filter((entry) => slotKey(entry) !== slotKey(row)))}><Trash2 size={12} /></button></td>
            </tr>)}</tbody>
          </table>}
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">An elements.data edit (one undo step there); save elements.data for the server to use it.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn primary" disabled={!pending.length || busy} onClick={() => void apply()}>{busy ? <Loader2 size={14} className="spin" /> : <Check size={14} />} Use for {count(pending.length)} monster{pending.length === 1 ? "" : "s"}</button>
      </footer>
    </div>
  </div>
    {picking && <ValuePicker search={(text, page) => pickEssence("monster", text, page, null)} onApply={async (value) => { const id = Number(value); if (!id) return "Choose a monster"; await add([id]); setPicking(false); return null; }} onClose={() => setPicking(false)} />}
  </>;
}
