import { useEffect, useMemo, useRef, useState } from "react";
import { CircleAlert, Loader2, Play, Search, X } from "lucide-react";
import { essenceRows, iconUrl, lotteryItems } from "../elements/api";
import { count } from "../elements/format";
import type { LuaScript } from "../elements/luaData";
import { LotterySandbox, type LotteryResult } from "../elements/luaRun";
import type { LotteryItem, RecordRow } from "../elements/types";

interface Props {
  text: string;
  path: string;
  script: LuaScript;
  /** Client icon generation (null: no client folder). */
  iconGeneration: number | null;
}

/** A lottery as the script and elements.data know it. */
interface Row {
  id: number;
  name: string;
  icon?: number;
  kind: string | null;
  dice: number | null;
  /** The script's own label (the comment on its functions: "Lottery Used@20070618(神木宝盒)"). */
  label: string | null;
  selectFn: string | null;
  cashingFn: string | null;
}

const percent = (value: number) => value >= 0.0001 ? `${(value * 100).toFixed(value >= 0.1 ? 2 : 3)}%` : value > 0 ? "<0.01%" : "0%";
const TIERS = ["", "1st prize", "2nd prize", "3rd prize", "4th prize", "5th prize", "6th prize"];

/** The script's dispatch: `if id==N then return LotteryTypeXSelectNumber(...)` in LotterySelectNumber / LotteryCashing. */
function dispatch(script: LuaScript, fn: string): Map<number, string> {
  const out = new Map<number, string>();
  for (const entry of script.returns) {
    if (entry.fn !== fn) continue;
    const id = /^(?:if|elseif) \(?\s*id\s*==\s*(\d+)\s*\)?$/.exec(entry.conditions[entry.conditions.length - 1] ?? "")?.[1];
    const target = /^(\w+)\s*\(/.exec(entry.values[0] ?? "")?.[1];
    if (id && target && !out.has(Number(id))) out.set(Number(id), target);
  }
  return out;
}

function ItemCell({ id, row, generation, elementsOpen }: { id: number; row?: RecordRow; generation: number | null; elementsOpen: boolean }) {
  return <span className="lua-item">
    {row?.icon !== undefined && generation !== null ? <img src={iconUrl(generation, row.icon)} alt="" /> : <span className="lua-item-blank" />}
    <span className="truncate" style={row?.color ? { color: row.color } : undefined}>{row?.name ?? (!id ? <span className="muted">nothing</span> : elementsOpen ? <span className="ai-warn">not in elements.data</span> : <span className="muted">open elements.data for the name</span>)}</span>
    {id > 0 && <span className="mono muted small">{id}</span>}
  </span>;
}

/** lottery.lua: every lottery item with its exact odds, worked out by running the script in a sandbox. */
export function LuaLotteryView({ text, path, script, iconGeneration }: Props) {
  const [items, setItems] = useState<LotteryItem[] | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Map<number, LotteryResult>>(new Map());
  const [names, setNames] = useState<Map<number, RecordRow>>(new Map());
  const [checking, setChecking] = useState<{ done: number; total: number } | null>(null);
  const [showSource, setShowSource] = useState(false);
  const [diceOverride, setDiceOverride] = useState<Record<number, number>>({});
  const sandbox = useRef<LotterySandbox | null>(null);

  useEffect(() => { lotteryItems().then(setItems).catch(() => setItems([])); }, []);
  // One sandbox per script text.
  useEffect(() => {
    const box = new LotterySandbox(text, path.split(/[\\/]/).pop());
    sandbox.current = box;
    setResults(new Map());
    return () => { box.dispose(); if (sandbox.current === box) sandbox.current = null; };
  }, [path, text]);

  const rows = useMemo(() => {
    const select = dispatch(script, "LotterySelectNumber");
    const cashing = dispatch(script, "LotteryCashing");
    const labels = new Map(script.functions.map((fn) => [fn.name, fn.comment]));
    const out = new Map<number, Row>();
    for (const item of items ?? []) out.set(item.id, { id: item.id, name: item.name, icon: item.icon, kind: item.kind, dice: item.dice, label: null, selectFn: null, cashingFn: null });
    for (const id of new Set([...select.keys(), ...cashing.keys()])) {
      const row = out.get(id) ?? { id, name: "", kind: null, dice: null, label: null, selectFn: null, cashingFn: null };
      row.selectFn = select.get(id) ?? null;
      row.cashingFn = cashing.get(id) ?? null;
      row.label = (row.selectFn && labels.get(row.selectFn)) || (row.cashingFn && labels.get(row.cashingFn)) || null;
      out.set(id, row);
    }
    return [...out.values()].sort((a, b) => a.id - b.id);
  }, [items, script]);
  const diceOf = (row: Row) => diceOverride[row.id] ?? row.dice ?? 1;

  const evaluate = (row: Row) => {
    const box = sandbox.current;
    if (!box) return null;
    const found = results.get(row.id);
    if (found && found.dice === diceOf(row)) return found;
    const result = box.evaluate(row.id, diceOf(row));
    setResults((current) => new Map(current).set(row.id, result));
    return result;
  };
  const current = rows.find((row) => row.id === selected) ?? null;
  const result = current ? results.get(current.id) : undefined;
  useEffect(() => {
    if (!current) return;
    const found = results.get(current.id);
    if (!found || found.dice !== diceOf(current)) evaluate(current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current?.id, diceOverride, results]);
  // Names and icons of the prizes shown.
  useEffect(() => {
    const ids = [...new Set((result?.outcomes ?? []).map((outcome) => outcome.item).filter((id) => id > 0 && !names.has(id)))];
    if (!ids.length) return;
    essenceRows(ids).then((found) => setNames((map) => { const next = new Map(map); for (const row of found) next.set(row.id, row); return next; })).catch(() => {});
  }, [result, names]);

  /** The server's start-up test for every lottery (in small steps, so the window stays responsive). */
  const checkAll = () => {
    const todo = rows.filter((row) => row.kind !== null);
    setChecking({ done: 0, total: todo.length });
    let index = 0;
    const step = () => {
      const box = sandbox.current;
      if (!box) { setChecking(null); return; }
      const batch: [number, LotteryResult][] = [];
      const end = Math.min(todo.length, index + 12);
      for (; index < end; index++) batch.push([todo[index].id, box.evaluate(todo[index].id, diceOf(todo[index]))]);
      setResults((map) => { const next = new Map(map); for (const [id, value] of batch) next.set(id, value); return next; });
      setChecking(index < todo.length ? { done: index, total: todo.length } : null);
      if (index < todo.length) window.setTimeout(step, 0);
    };
    window.setTimeout(step, 0);
  };
  const failed = rows.filter((row) => results.get(row.id)?.error);
  const visible = rows.filter((row) => {
    const text = query.trim().toLowerCase();
    return !text || String(row.id) === text || row.name.toLowerCase().includes(text) || (row.label ?? "").toLowerCase().includes(text);
  });
  const lines = useMemo(() => text.split(/\r?\n/), [text]);
  const source = (name: string | null) => {
    const fn = name ? script.functions.find((entry) => entry.name === name) : null;
    return fn ? lines.slice(fn.line - 1, fn.endLine).join("\n") : null;
  };
  const tiers = useMemo(() => {
    const out = new Map<number, LotteryResult["outcomes"]>();
    for (const outcome of result?.outcomes ?? []) out.set(outcome.tier, [...(out.get(outcome.tier) ?? []), outcome]);
    return [...out.entries()].sort((a, b) => a[0] - b[0]);
  }, [result]);

  return <div className="lua-split">
    <aside className="dyn-task-list">
      <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Lottery name or ID" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
      <div className="dyn-task-rows">
        {visible.map((row) => {
          const state = results.get(row.id);
          return <button key={row.id} className={"dyn-task-row lua-row" + (row.id === selected ? " selected" : "")} onClick={() => setSelected(row.id)}>
            {row.icon !== undefined && iconGeneration !== null ? <img className="lua-row-icon" src={iconUrl(iconGeneration, row.icon)} alt="" /> : <span className="lua-row-icon" />}
            <span className="gshop-row-text"><span className="truncate">{row.name || row.label || `Lottery ${row.id}`}</span><span className="muted small truncate">{row.id}{row.kind ? ` · ${row.dice ?? "?"} digit${row.dice === 1 ? "" : "s"}` : " · not in elements.data"}</span></span>
            {state?.error || (row.kind && !row.selectFn && !row.cashingFn) ? <CircleAlert size={12} className="danger-icon" /> : !row.kind ? <span className="tag">script only</span> : <span />}
          </button>;
        })}
        {!visible.length && <div className="empty-note">{items === null ? "Reading…" : "No lottery."}</div>}
      </div>
      <footer className="ai-list-foot">
        <span className="muted small">{count(rows.length)} lotteries{failed.length ? ` · ${failed.length} fail` : ""}</span>
        <button className="btn small" onClick={checkAll} disabled={!!checking || !items?.length} title="Run the server's start-up test (draw the digits, cash them) for every lottery item of elements.data">{checking ? <><Loader2 size={12} className="spin" /> {checking.done}/{checking.total}</> : <><Play size={12} /> Check all</>}</button>
      </footer>
    </aside>
    <div className="dyn-task-form-scroll drops-form">
      {items !== null && !items.length && <div className="path-data-message">Open the server's elements.data for the lottery items' names, icons and digit counts (dice_count); without it every lottery is drawn with 1 digit.</div>}
      {failed.length > 0 && !current && <section className="dyn-section"><header><b>Lotteries the server disables</b><span className="muted small">gs tests each lottery item at start-up; when the script fails, the item is not created</span></header>
        <div className="dyn-section-body"><ul className="lua-fail-list">{failed.map((row) => <li key={row.id}><button className="link" onClick={() => setSelected(row.id)}>{row.name || row.id}</button> <span className="mono muted small">{row.id}</span> — <span className="ai-warn small">{results.get(row.id)?.error}</span></li>)}</ul></div>
      </section>}
      {!current ? <div className="empty-note">Select a lottery. “Check all” runs the server's start-up test for every lottery item.</div> : <>
        <section className="dyn-section">
          <header><b>{current.name || `Lottery ${current.id}`}</b><span className="mono muted small">{current.id}</span>{current.kind && <span className="tag">{current.kind.replace("_ESSENCE", "")}</span>}<span className="spacer" />
            <label className="small lua-dice">Digits <input className="dyn-input mono" type="number" min={1} max={16} value={diceOf(current)} onChange={(event) => setDiceOverride((map) => ({ ...map, [current.id]: Math.max(1, Math.min(16, Number(event.target.value) || 1)) }))} title={current.dice !== null ? `dice_count in elements.data: ${current.dice}` : "Not in elements.data: choose how many digits a ticket draws"} /></label>
          </header>
          <div className="dyn-section-body">
            {current.label && <div className="muted small">Script label: {current.label}</div>}
            {!current.selectFn && !current.cashingFn && <div className="path-data-message error"><CircleAlert size={13} /> lottery.lua has no entry for this item: the server's start-up test fails and the item is not created.</div>}
            {result?.error && (current.selectFn || current.cashingFn) && <div className="path-data-message error"><CircleAlert size={13} /> The script fails: {result.error}. The server's start-up test fails and the item is not created.</div>}
            {result && !result.error && <div className="muted small">{result.exact ? `Exact odds over ${count(result.runs)} draw paths.` : `Estimated from ${count(result.runs)} sample draws (the script uses plain random numbers or has too many paths).`} Digits are drawn one by one by {current.selectFn ?? "LotterySelectNumber"}; {current.cashingFn ?? "LotteryCashing"} turns them into the prize.</div>}
          </div>
        </section>
        {result && !result.error && <>
          <section className="dyn-section">
            <header><b>Digits</b><span className="muted small">the chance of each number per drawn digit</span></header>
            <div className="dyn-section-body lua-digits">
              {result.digits.map((values, position) => <div key={position} className="lua-digit"><span className="muted small">Digit {position + 1}</span>{values.map((entry) => <span key={entry.value} className="lua-chip"><b className="mono">{entry.value}</b> {percent(entry.chance)}</span>)}</div>)}
            </div>
          </section>
          <section className="dyn-section">
            <header><b>Prizes</b><span className="muted small">{count(result.outcomes.length)} outcome{result.outcomes.length === 1 ? "" : "s"}; the chance of each per opened item</span></header>
            <div className="dyn-section-body">
              <table className="dyn-table lua-prizes">
                <thead><tr><th>Item</th><th>Count</th><th>Extra</th><th>Chance</th></tr></thead>
                {tiers.map(([tier, outcomes]) => <tbody key={tier}>
                  <tr className="lua-tier"><td colSpan={3}><b>{TIERS[tier] ?? `Tier ${tier}`}</b> <span className="muted small">(tier {tier})</span></td><td className="mono"><b>{percent(outcomes.reduce((sum, outcome) => sum + outcome.chance, 0))}</b></td></tr>
                  {outcomes.map((outcome, index) => <tr key={index}>
                    <td><ItemCell id={outcome.item} row={names.get(outcome.item)} generation={iconGeneration} elementsOpen={!!items?.length} /></td>
                    <td className="mono">{outcome.count}</td>
                    <td className="small muted">{[outcome.money ? `${count(outcome.money)} money` : "", outcome.bind ? "bound" : "", outcome.period ? `${count(outcome.period)} s limit` : ""].filter(Boolean).join(", ")}</td>
                    <td className="mono">{percent(outcome.chance)}</td>
                  </tr>)}
                </tbody>)}
              </table>
            </div>
          </section>
        </>}
        <section className="dyn-section">
          <header><b>Script</b><span className="muted small">{[current.selectFn, current.cashingFn].filter(Boolean).join(", ") || "none"}</span><span className="spacer" /><button className="btn small" onClick={() => setShowSource((value) => !value)}>{showSource ? "Hide" : "Show"}</button></header>
          {showSource && <div className="dyn-section-body">{[current.selectFn, current.cashingFn].map((name) => name && <pre key={name} className="lua-source">{source(name) ?? `${name} is not defined in this file`}</pre>)}</div>}
        </section>
      </>}
    </div>
  </div>;
}
