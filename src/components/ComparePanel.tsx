import { useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ArrowLeftRight, Check, ChevronRight, ClipboardCopy, FileDiff, FolderOpen, GitCompareArrows, Loader2, X } from "lucide-react";
import { closeCompare, compareList, compareMarkdown, openCompare } from "../elements/api";
import type { ChangedRecord, CompareFile, ComparePair, CompareRecord, CompareSummary, ListDiff } from "../elements/types";
import { bytes as fmtBytes, count } from "../elements/format";

interface Props {
  /** The open file, to suggest comparing with the client's or the last one. */
  currentPath: string;
  suggestions: string[];
  icon?: (pathId?: number | null) => string | undefined;
  /** Open a record of the open file (removed records only exist in the other one). */
  onOpen: (list: number, row: number, newTab: boolean) => void;
  onClose: () => void;
}

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;
const date = (t: number) => new Date(t * 1000).toLocaleDateString();
/** Every file is "elements.data": show the folders that tell them apart. */
const shortPath = (p: string) => p.split(/[\\/]/).filter(Boolean).slice(-4).join("/");

/** Compares the open file with another: records added, removed and changed, per list. */
export function ComparePanel({ currentPath, suggestions, icon, onOpen, onClose }: Props) {
  const [summary, setSummary] = useState<CompareSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Patch notes read from old to new: is the other file the older one?
  const [otherOlder, setOtherOlder] = useState(true);
  const [onlyChanged, setOnlyChanged] = useState(true);
  const [query, setQuery] = useState("");
  const [openList, setOpenList] = useState<string | null>(null);
  const [diffs, setDiffs] = useState<Record<string, ListDiff | "loading">>({});
  const [copied, setCopied] = useState(false);

  const start = async (path: string) => {
    setBusy(true);
    setError(null);
    try {
      setSummary(await openCompare(path));
      setDiffs({});
      setOpenList(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const choose = async () => {
    const picked = await openDialog({ multiple: false, directory: false, filters: [{ name: "Element data", extensions: ["data"] }, { name: "All files", extensions: ["*"] }] });
    if (typeof picked === "string") start(picked);
  };

  const stop = () => {
    closeCompare().catch(() => {});
    setSummary(null);
    setDiffs({});
  };

  const keyOf = (p: ComparePair) => `${p.this}:${p.other}`;
  const toggle = (p: ComparePair) => {
    const key = keyOf(p);
    if (openList === key) return setOpenList(null);
    setOpenList(key);
    if (!diffs[key]) {
      setDiffs((d) => ({ ...d, [key]: "loading" }));
      compareList(p)
        .then((diff) => setDiffs((d) => ({ ...d, [key]: diff })))
        .catch((e) => setError(String(e)));
    }
  };

  const copyNotes = async () => {
    const md = await compareMarkdown(otherOlder);
    await navigator.clipboard.writeText(md);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  // "Added" means: in the newer file only.
  const added = (p: ComparePair) => (otherOlder ? p.onlyThis : p.onlyOther);
  const removed = (p: ComparePair) => (otherOlder ? p.onlyOther : p.onlyThis);

  const q = query.trim().toLowerCase();
  const pairs = useMemo(
    () =>
      (summary?.lists ?? []).filter(
        (p) => (!onlyChanged || p.onlyThis + p.onlyOther + p.changed > 0) && (!q || p.name.toLowerCase().includes(q) || (p.structName ?? "").toLowerCase().includes(q)),
      ),
    [summary, onlyChanged, q],
  );
  const totals = useMemo(() => {
    const t = { added: 0, removed: 0, changed: 0, lists: 0 };
    for (const p of summary?.lists ?? []) {
      t.added += added(p);
      t.removed += removed(p);
      t.changed += p.changed;
      t.lists += Number(p.onlyThis + p.onlyOther + p.changed > 0);
    }
    return t;
  }, [summary, otherOlder]);

  const head = (
    <div className="pane-head">
      <span className="pane-title">
        <GitCompareArrows size={14} /> Compare files
      </span>
      <span className="spacer" />
      {summary && (
        <button className="link" onClick={stop} title="Stop comparing and free the other file">
          Stop comparing
        </button>
      )}
      <button className="icon-btn small" onClick={onClose} title="Close (Esc)" aria-label="Close">
        <X size={15} />
      </button>
    </div>
  );

  if (!summary) {
    const others = suggestions.filter((s) => s && s !== currentPath);
    return (
      <section className="pane search-panel compare-panel" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        {head}
        <div className="compare-start">
          <FileDiff size={30} className="muted" />
          <p>
            Compare <b>{fileName(currentPath)}</b> with another elements.data: another version, or the server's file against the client's. Lists are paired by struct, records by ID, fields by name.
          </p>
          <button className="btn primary" onClick={choose} disabled={busy}>
            {busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose a file…
          </button>
          {others.map((p) => (
            <button key={p} className="link" onClick={() => start(p)} disabled={busy} title={p}>
              Compare with {shortPath(p)}
            </button>
          ))}
          {error && <div className="se-problems">{error}</div>}
        </div>
      </section>
    );
  }

  const [older, newer] = otherOlder ? [summary.other, summary.this] : [summary.this, summary.other];
  const side = (f: CompareFile, label: string, current: boolean) => (
    <div className="compare-file" title={f.path}>
      <span className="sf-label">
        {label} {current && <span className="tag">open</span>}
      </span>
      <span className="truncate">{shortPath(f.path)}</span>
      <span className="muted small">
        v{f.version} · exported {date(f.timestamp)} · {fmtBytes(f.fileSize)} · {count(f.records)} records
      </span>
    </div>
  );

  return (
    <section className="pane search-panel compare-panel" onKeyDown={(e) => e.key === "Escape" && onClose()}>
      {head}
      <div className="problems-tools">
        <div className="compare-files">
          {side(older, "Before", older === summary.this)}
          <button className="icon-btn small" onClick={() => setOtherOlder((o) => !o)} title="Swap which file is the older one (before → after)">
            <ArrowLeftRight size={15} />
          </button>
          {side(newer, "After", newer === summary.this)}
        </div>
        <div className="compare-totals">
          <span className="diff-add">+{count(totals.added)} added</span>
          <span className="diff-del">−{count(totals.removed)} removed</span>
          <span className="diff-mod">~{count(totals.changed)} changed</span>
          <span className="muted">in {totals.lists} lists</span>
          {summary.talks.some(Boolean) && (
            <span className="muted" title="NPC dialogs: added, removed, changed">
              · dialogs +{otherOlder ? summary.talks[0] : summary.talks[1]} −{otherOlder ? summary.talks[1] : summary.talks[0]} ~{summary.talks[2]}
            </span>
          )}
          <span className="spacer" />
          <button className="link" onClick={copyNotes} title="Copy every difference as Markdown, for patch notes">
            {copied ? <Check size={12} /> : <ClipboardCopy size={12} />} Copy patch notes
          </button>
        </div>
        <div className="coverage-controls">
          <input className="sf-control" placeholder="Filter lists…" value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
          <label className="check">
            <input type="checkbox" checked={onlyChanged} onChange={(e) => setOnlyChanged(e.target.checked)} /> Only lists with changes
          </label>
        </div>
        {error && <div className="se-problems">{error}</div>}
      </div>

      <div className="search-results scroll">
        {pairs.length === 0 && <div className="empty-note">{onlyChanged ? "The files hold the same records." : "No lists match."}</div>}
        {pairs.map((p) => {
          const key = keyOf(p);
          const isOpen = openList === key;
          const diff = diffs[key];
          const missing = p.this === null ? "only in the other file" : p.other === null ? "only in this file" : null;
          return (
            <div key={key} className="search-group">
              <button className="search-group-head compare-head" onClick={() => toggle(p)} title={p.structName}>
                <ChevronRight size={14} className={"caret-icon" + (isOpen ? " open" : "")} />
                <span className="truncate">{p.name}</span>
                {missing && <span className="tag warn">{missing}</span>}
                {p.thisSize !== p.otherSize && p.thisSize && p.otherSize ? (
                  <span className="muted small" title="Record size: before → after">
                    {otherOlder ? p.otherSize : p.thisSize} → {otherOlder ? p.thisSize : p.otherSize} B
                  </span>
                ) : null}
                <span className="spacer" />
                {added(p) > 0 && <span className="diff-add small">+{count(added(p))}</span>}
                {removed(p) > 0 && <span className="diff-del small">−{count(removed(p))}</span>}
                {p.changed > 0 && <span className="diff-mod small">~{count(p.changed)}</span>}
              </button>
              {isOpen && (diff === "loading" || !diff ? <div className="empty-note">Comparing…</div> : <ListDiffView diff={diff} pair={p} otherOlder={otherOlder} icon={icon} onOpen={onOpen} />)}
            </div>
          );
        })}
      </div>
    </section>
  );
}

function ListDiffView({
  diff,
  pair,
  otherOlder,
  icon,
  onOpen,
}: {
  diff: ListDiff;
  pair: ComparePair;
  otherOlder: boolean;
  icon?: (pathId?: number | null) => string | undefined;
  onOpen: (list: number, row: number, newTab: boolean) => void;
}) {
  const [openRecord, setOpenRecord] = useState<number | null>(null);
  const [addedRecs, removedRecs] = otherOlder ? [diff.onlyThis, diff.onlyOther] : [diff.onlyOther, diff.onlyThis];
  // Records of this (the open) file can be opened.
  const inThis = (r: CompareRecord) => (otherOlder ? addedRecs : removedRecs).includes(r);

  const record = (r: CompareRecord, kind: "add" | "del", clickable: boolean) => (
    <button
      key={`${kind}${r.row}`}
      className={"search-hit compare-rec" + (clickable ? "" : " static")}
      onClick={(e) => clickable && pair.this !== null && onOpen(pair.this, r.row, e.ctrlKey || e.metaKey)}
      disabled={!clickable}
      title={clickable ? "Open (Ctrl+click: new tab)" : "Only in the other file"}
    >
      <span className="find-icon">{icon?.(r.icon) ? <img src={icon(r.icon)} alt="" draggable={false} /> : null}</span>
      <span className="truncate">
        <span className={kind === "add" ? "diff-add" : "diff-del"}>{kind === "add" ? "+ " : "− "}</span>
        {r.name || <span className="muted">#{r.row}</span>}
      </span>
      <span className="mono muted small">{r.id}</span>
    </button>
  );

  const changed = (c: ChangedRecord) => {
    const isOpen = openRecord === c.row;
    return (
      <div key={`c${c.row}`}>
        <div className="search-hit compare-rec">
          <span className="find-icon">{icon?.(c.icon) ? <img src={icon(c.icon)} alt="" draggable={false} /> : null}</span>
          <button className="compare-rec-main" onClick={() => setOpenRecord(isOpen ? null : c.row)} title="Show the changed fields">
            <ChevronRight size={12} className={"caret-icon" + (isOpen ? " open" : "")} />
            <span className="truncate">
              <span className="diff-mod">~ </span>
              {c.name || <span className="muted">#{c.row}</span>}
              {c.otherName !== undefined && <span className="muted small"> (other: {c.otherName || "—"})</span>}
            </span>
            <span className="muted small">
              {c.fields.length} field{c.fields.length === 1 ? "" : "s"}
            </span>
          </button>
          {pair.this !== null && (
            <button className="link mono small" onClick={(e) => onOpen(pair.this!, c.row, e.ctrlKey || e.metaKey)} title="Open in this file (Ctrl+click: new tab)">
              {c.id}
            </button>
          )}
        </div>
        {isOpen && (
          <table className="field-diff">
            <thead>
              <tr>
                <th>Field</th>
                <th>Before</th>
                <th>After</th>
              </tr>
            </thead>
            <tbody>
              {c.fields.map((f) => {
                const [before, after] = otherOlder ? [f.other, f.this] : [f.this, f.other];
                return (
                  <tr key={f.field}>
                    <td className="mono">{f.field}</td>
                    <td className="diff-old">{before ?? <span className="muted">—</span>}</td>
                    <td className="diff-new">{after ?? <span className="muted">—</span>}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    );
  };

  return (
    <div className="list-diff">
      {addedRecs.map((r) => record(r, "add", inThis(r)))}
      {removedRecs.map((r) => record(r, "del", inThis(r)))}
      {diff.changed.map(changed)}
      {diff.truncated && <div className="search-note muted small">Showing the first {count(diff.changed.length)} changed records.</div>}
    </div>
  );
}
