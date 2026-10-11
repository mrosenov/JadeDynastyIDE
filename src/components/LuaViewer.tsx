import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Braces, FolderOpen, Loader2, Search, X } from "lucide-react";
import { readLuaFile } from "../elements/api";
import { count } from "../elements/format";
import { readLuaScript, showValue, type LuaEntry, type LuaScript } from "../elements/luaData";
import type { LuaFile } from "../elements/types";
import { LuaLotteryView } from "./LuaLotteryView";
import { PetGradesView, PetListView, PetSkillsView, petDictionary, prettyCondition, type PetDictionary } from "./LuaPetViews";

export interface LuaViewerState {
  loaded: boolean;
  path: string | null;
}

export interface LuaViewerHandle {
  choose: () => void;
  openPath: (path: string) => void;
}

interface Props {
  active: boolean;
  onStateChange: (state: LuaViewerState) => void;
  iconGeneration: number | null;
}

type Rule = "lottery" | "pets" | "grades" | "skills" | null;
const LAST_KEY = "jdide.lua.lastPath";
const RULE_LABELS: Record<Exclude<Rule, null>, string> = { lottery: "Lotteries", pets: "Pets", grades: "Pet grades", skills: "Pet skills" };

/** The viewer a script gets by its file name. */
function ruleOf(path: string, script: LuaScript): Rule {
  const name = path.split(/[\\/]/).pop()?.toLowerCase() ?? "";
  if (name.includes("lottery") && script.functions.some((fn) => fn.name === "LotteryCashing" || fn.name === "LotterySelectNumber")) return "lottery";
  if (name === "petbedge_interface.lua") return "pets";
  if (name === "petbedge_grades.lua") return "grades";
  if (name === "petbedge_skills.lua") return "skills";
  return null;
}

/** Every literal data assignment of a script, by function and under its conditions. */
function DataView({ script, dictionary }: { script: LuaScript; dictionary: PetDictionary | null }) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const byFunction = useMemo(() => {
    const out = new Map<string, LuaEntry[]>();
    for (const entry of script.entries) out.set(entry.fn, [...(out.get(entry.fn) ?? []), entry]);
    return out;
  }, [script]);
  const functions = useMemo(() => [{ name: "", line: 1, comment: "Assignments outside functions" }, ...script.functions].filter((fn) => byFunction.has(fn.name)), [byFunction, script]);
  const text = query.trim().toLowerCase();
  const shown = functions.filter((fn) => !text || fn.name.toLowerCase().includes(text) || (fn.comment ?? "").toLowerCase().includes(text) || (byFunction.get(fn.name) ?? []).some((entry) => entry.target.toLowerCase().includes(text) || showValue(entry.value).toLowerCase().includes(text)));
  const current = selected !== null && byFunction.has(selected) ? selected : shown[0]?.name ?? null;
  const entries = current !== null ? byFunction.get(current) ?? [] : [];
  const condition = (text: string) => text === "else" ? "otherwise" : dictionary ? prettyCondition(text, dictionary, () => undefined) : text.replace(/^(if|elseif) /, "");
  return <div className="lua-split">
    <aside className="dyn-task-list">
      <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Function, name or value" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
      <div className="dyn-task-rows">
        {shown.map((fn) => <button key={fn.name || "(top)"} className={"dyn-task-row lua-fn-row" + (fn.name === current ? " selected" : "")} onClick={() => setSelected(fn.name)}>
          <span className="gshop-row-text"><span className="truncate mono">{fn.name || "(top level)"}</span><span className="muted small truncate">{fn.comment ?? `line ${fn.line}`}</span></span>
          <span className="muted small">{byFunction.get(fn.name)?.length}</span>
        </button>)}
        {!shown.length && <div className="empty-note">Nothing matches.</div>}
      </div>
      <footer className="ai-list-foot"><span className="muted small">{count(shown.length)} functions with data · {count(script.entries.length)} values</span></footer>
    </aside>
    <div className="dyn-task-form-scroll drops-form">
      {current === null ? <div className="empty-note">This script has no literal data.</div> : <section className="dyn-section">
        <header><b className="mono">{current || "(top level)"}</b><span className="muted small">{script.functions.find((fn) => fn.name === current)?.comment ?? ""}</span></header>
        <div className="dyn-section-body lua-wide"><table className="dyn-table lua-table lua-data">
          <thead><tr><th>When</th><th>Name</th><th>Value</th><th>Note</th><th>Line</th></tr></thead>
          <tbody>{entries.map((entry, index) => <tr key={index}>
            <td className="small">{entry.conditions.length ? entry.conditions.map((text, at) => <div key={at} className="lua-condition">{condition(text)}</div>) : <span className="muted">always</span>}</td>
            <td className="mono small">{entry.local ? "local " : ""}{entry.target}</td>
            <td className="mono small lua-value">{showValue(entry.value)}</td>
            <td className="small">{entry.comment ?? ""}</td>
            <td className="mono muted small">{entry.line}</td>
          </tr>)}</tbody>
        </table></div>
      </section>}
    </div>
  </div>;
}

/** Server Lua scripts, read-only: views by file name (lotteries, pets) and every literal value of any script. */
export const LuaViewer = forwardRef<LuaViewerHandle, Props>(function LuaViewer({ active, onStateChange, iconGeneration }, ref) {
  const [file, setFile] = useState<LuaFile | null>(null);
  const [interfaceScript, setInterfaceScript] = useState<LuaScript | null>(null);
  const [tab, setTab] = useState<"rule" | "data">("rule");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const script = useMemo(() => file ? readLuaScript(file.text) : null, [file]);
  const rule = file && script ? ruleOf(file.path, script) : null;

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    try {
      const read = await readLuaFile(path);
      setFile(read);
      setTab("rule");
      try { localStorage.setItem(LAST_KEY, read.path); } catch { /* optional */ }
      // Pet scripts name their keys in petbedge_interface.lua next to them.
      const name = read.path.split(/[\\/]/).pop()?.toLowerCase() ?? "";
      setInterfaceScript(null);
      if (name.startsWith("petbedge") && name !== "petbedge_interface.lua") {
        readLuaFile(read.path.replace(/[^\\/]+$/, "petbedge_interface.lua")).then((other) => setInterfaceScript(readLuaScript(other.text))).catch(() => {});
      }
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  }, []);
  useEffect(() => {
    let last: string | null = null;
    try { last = localStorage.getItem(LAST_KEY); } catch { /* optional */ }
    if (last) void load(last);
  }, [load]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: file?.path, title: "Open a server Lua script (gamed or gamed\\script)", filters: [{ name: "Lua scripts", extensions: ["lua"] }, { name: "All files", extensions: ["*"] }] });
    if (typeof picked === "string") void load(picked);
  }, [file?.path, load]);
  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => void load(path) }), [choose, load]);
  useEffect(() => onStateChange({ loaded: !!file, path: file?.path ?? null }), [file, onStateChange]);
  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "o") { event.preventDefault(); event.stopImmediatePropagation(); void choose(); }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, choose]);

  const dictionary = useMemo(() => rule === "pets" ? petDictionary(script) : interfaceScript ? petDictionary(interfaceScript) : rule === "grades" || rule === "skills" ? petDictionary(null) : null, [interfaceScript, rule, script]);

  if (!file || !script) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <Braces size={30} />
      <h2>Open a server Lua script</h2>
      <p className="muted">The game server's scripts (<span className="mono">gamed</span> and <span className="mono">gamed\script</span>), read-only. lottery.lua shows every lottery's exact odds; the petbedge scripts show pets, grades and skills; any other script lists its data values.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose a .lua file…</button>
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  const lines = file.text.split(/\r?\n/).length;
  return <section className="path-data-pane lua-pane">
    <header className="path-data-head">
      <div>
        <h2>{rule ? RULE_LABELS[rule] : "Lua script"} · {file.path.split(/[\\/]/).pop()}</h2>
        <div className="path-data-file-line">
          <span className="path-data-location mono muted small truncate" title={file.path}>{file.path}</span>
          <span className="path-data-badge"><b>Lines:</b> {count(lines)}</span>
          <span className="path-data-badge"><b>Functions:</b> {count(script.functions.length)}</span>
          <span className="path-data-badge"><b>Text:</b> {file.encoding === "gbk" ? "GBK" : "UTF-8"}</span>
        </div>
      </div>
      {rule && <span className="quiz-view-tabs">
        <button className={"btn small" + (tab === "rule" ? " active" : "")} onClick={() => setTab("rule")}>{RULE_LABELS[rule]}</button>
        <button className={"btn small" + (tab === "data" ? " active" : "")} onClick={() => setTab("data")}>All data</button>
      </span>}
      <button className="btn nowrap" onClick={() => void choose()} disabled={busy}><FolderOpen size={13} /> Open…</button>
    </header>
    {script.error && <div className="path-data-message error"><AlertTriangle size={14} /> Not valid Lua: {script.error}. The game server stops loading its scripts here (it exits with -11).</div>}
    {error && <div className="path-data-message error"><AlertTriangle size={14} /> {error}</div>}
    {(rule === "grades" || rule === "skills") && !interfaceScript && <div className="path-data-message">petbedge_interface.lua is not next to this file: pets are shown by their script keys.</div>}
    <div className="lua-body">
      {rule === "lottery" && tab === "rule" ? <LuaLotteryView text={file.text} path={file.path} script={script} iconGeneration={iconGeneration} />
        : rule === "pets" && tab === "rule" ? <PetListView script={script} iconGeneration={iconGeneration} />
        : rule === "grades" && tab === "rule" && dictionary ? <PetGradesView script={script} dictionary={dictionary} />
        : rule === "skills" && tab === "rule" && dictionary ? <PetSkillsView script={script} dictionary={dictionary} />
        : <DataView script={script} dictionary={dictionary} />}
    </div>
  </section>;
});
