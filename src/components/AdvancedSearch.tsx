import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronRight, CircleAlert, Copy, Hash, ListFilter, Loader2, Plus, Search, Trash2, X } from "lucide-react";
import { namedSet, searchFieldNames, searchRecords } from "../elements/api";
import type { ListSummary, SearchCondition, SearchFieldName, SearchHit, SearchOp, SearchQuery, SearchReport } from "../elements/types";
import { count } from "../elements/format";

interface Props {
  lists: ListSummary[];
  /** The list open in the active tab (for "this list"). */
  currentList: number | null;
  icon?: (pathId?: number | null) => string | undefined;
  /** Open a hit, at the matched field's offset. */
  onOpen: (hit: SearchHit, offset: number | null, newTab: boolean) => void;
  onClose: () => void;
}

type Mode = "conditions" | "value";
type ValueKind = "int" | "float" | "text" | "hex";

const OPS: { op: SearchOp; label: string; kinds: SearchFieldName["kind"][]; value: boolean }[] = [
  { op: "eq", label: "=", kinds: ["int", "float", "text", "bytes"], value: true },
  { op: "ne", label: "≠", kinds: ["int", "float", "text", "bytes"], value: true },
  { op: "lt", label: "<", kinds: ["int", "float"], value: true },
  { op: "le", label: "≤", kinds: ["int", "float"], value: true },
  { op: "gt", label: ">", kinds: ["int", "float"], value: true },
  { op: "ge", label: "≥", kinds: ["int", "float"], value: true },
  { op: "in", label: "is one of", kinds: ["int", "float"], value: true },
  { op: "not_in", label: "is none of", kinds: ["int", "float"], value: true },
  { op: "has_flags", label: "has flags", kinds: ["int"], value: true },
  { op: "lacks_flags", label: "lacks flags", kinds: ["int"], value: true },
  { op: "contains", label: "contains", kinds: ["text", "int", "bytes"], value: true },
  { op: "starts", label: "starts with", kinds: ["text", "bytes"], value: true },
  { op: "ends", label: "ends with", kinds: ["text", "bytes"], value: true },
  { op: "empty", label: "is empty / 0", kinds: ["int", "float", "text", "bytes"], value: false },
  { op: "not_empty", label: "is not empty", kinds: ["int", "float", "text", "bytes"], value: false },
];

const PLACEHOLDER: Record<ValueKind, string> = {
  int: "e.g. 1291 or 0x50B",
  float: "e.g. 1.5",
  text: "e.g. Sword",
  hex: "e.g. 0B 05 00 00",
};

const newCondition = (): SearchCondition => ({ field: "", op: "eq", value: "" });

/** Searches the whole file by conditions on fields, or by a value in any field. */
export function AdvancedSearch({ lists, currentList, icon, onOpen, onClose }: Props) {
  const [mode, setMode] = useState<Mode>("conditions");
  const [scope, setScope] = useState<number | null>(null);
  const [conditions, setConditions] = useState<SearchCondition[]>([newCondition()]);
  const [matchAll, setMatchAll] = useState(true);
  const [value, setValue] = useState("");
  const [kind, setKind] = useState<ValueKind>("int");
  const [includeUnknown, setIncludeUnknown] = useState(false);
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [report, setReport] = useState<SearchReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [names, setNames] = useState<SearchFieldName[]>([]);
  const [labels, setLabels] = useState<Record<string, string[]>>({});
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const [copied, setCopied] = useState(false);
  const [active, setActive] = useState<string | null>(null);
  const firstField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    searchFieldNames().then(setNames).catch(() => setNames([]));
    firstField.current?.focus();
  }, []);

  const byName = useMemo(() => new Map(names.map((n) => [n.name, n])), [names]);
  const fieldInfo = (field: string) => byName.get(field.trim().toLowerCase());

  // Labels of the enums and masks the conditions use, for value suggestions.
  useEffect(() => {
    for (const c of conditions) {
      const set = fieldInfo(c.field)?.set;
      if (!set || labels[set]) continue;
      setLabels((l) => ({ ...l, [set]: [] }));
      namedSet(set)
        .then((d) => setLabels((l) => ({ ...l, [set]: (d.set.values ?? d.set.flags ?? []).map((v) => v.label) })))
        .catch(() => {});
    }
  }, [conditions, byName]);

  const update = (i: number, patch: Partial<SearchCondition>) =>
    setConditions((cs) => cs.map((c, j) => (j === i ? { ...c, ...patch } : c)));

  const run = async () => {
    const query: SearchQuery =
      mode === "conditions"
        ? { mode, conditions, matchAll, list: scope }
        : { mode, value, kind, list: scope, includeUnknown, caseSensitive };
    setBusy(true);
    setError(null);
    try {
      setReport(await searchRecords(query));
      setCollapsed(new Set());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && e.target instanceof HTMLInputElement && e.target.type !== "checkbox") {
      e.preventDefault();
      run();
    } else if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
    }
  };

  const groups = useMemo(() => {
    const out = new Map<number, SearchHit[]>();
    for (const h of report?.hits ?? []) out.set(h.list, [...(out.get(h.list) ?? []), h]);
    return [...out];
  }, [report]);

  const copyIds = () => {
    const ids = [...new Set((report?.hits ?? []).map((h) => h.id))];
    navigator.clipboard.writeText(ids.join("\n"));
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  };

  const open = (h: SearchHit, e: React.MouseEvent) => {
    const m = h.matches[0];
    setActive(`${h.list}:${h.row}`);
    onOpen(h, m?.off ?? null, e.ctrlKey || e.metaKey || e.button === 1);
  };

  return (
    <section className="pane search-panel" onKeyDown={onKey}>
      <div className="pane-head">
        <span className="pane-title">
          <Search size={14} /> Advanced search
        </span>
        <span className="spacer" />
        <button className="icon-btn small" onClick={onClose} title="Close the search (Esc)" aria-label="Close">
          <X size={15} />
        </button>
      </div>

      <div className="search-form">
        <div className="sf-modes" role="tablist" aria-label="Search by">
          <button role="tab" aria-selected={mode === "conditions"} className={mode === "conditions" ? "active" : ""} onClick={() => setMode("conditions")}>
            <ListFilter size={14} /> Conditions on fields
          </button>
          <button role="tab" aria-selected={mode === "value"} className={mode === "value" ? "active" : ""} onClick={() => setMode("value")}>
            <Hash size={14} /> Value in any field
          </button>
        </div>

        <label className="sf-field">
          <span className="sf-label">Look in</span>
          <select className="sf-control" value={scope ?? ""} onChange={(e) => setScope(e.target.value === "" ? null : Number(e.target.value))}>
            <option value="">All lists</option>
            {currentList !== null && currentList >= 0 && <option value={currentList}>This list: {lists[currentList]?.name}</option>}
            <optgroup label="One list">
              {lists
                .filter((l) => l.count > 0)
                .map((l) => (
                  <option key={l.index} value={l.index}>
                    {l.index} · {l.name}
                  </option>
                ))}
            </optgroup>
          </select>
        </label>

        {mode === "conditions" ? (
          <div className="sf-conditions">
            <datalist id="search-fields">
              {names.slice(0, 2000).map((n) => (
                <option key={n.name} value={n.name}>
                  {n.kind}
                  {n.set ? ` · ${n.set}` : ""} · {n.lists} list{n.lists === 1 ? "" : "s"}
                </option>
              ))}
            </datalist>
            <div className="sf-cond-head">
              <span />
              <span className="sf-label">Field</span>
              <span className="sf-label">Operator</span>
              <span className="sf-label">Value</span>
              <span />
            </div>
            {conditions.map((c, i) => {
              const info = fieldInfo(c.field);
              const ops = OPS.filter((o) => !info || o.kinds.includes(info.kind));
              const needsValue = OPS.find((o) => o.op === c.op)?.value ?? true;
              const valueLabels = info?.set ? labels[info.set] : undefined;
              const unknownField = c.field.trim() !== "" && names.length > 0 && !info;
              return (
                <div className="sf-cond" key={i}>
                  <span className={"sf-join" + (i === 0 ? " first" : "")}>{i === 0 ? "Where" : matchAll ? "And" : "Or"}</span>
                  <div className="sf-field-cell">
                    <input
                      ref={i === 0 ? firstField : undefined}
                      className={"sf-control mono" + (unknownField ? " invalid" : "")}
                      list="search-fields"
                      placeholder="e.g. proc_type"
                      value={c.field}
                      onChange={(e) => {
                        const next = fieldInfo(e.target.value);
                        // Keep the operator if it suits the new field.
                        const fits = !next || OPS.find((o) => o.op === c.op)?.kinds.includes(next.kind);
                        update(i, { field: e.target.value, ...(fits ? {} : { op: next!.kind === "text" ? "contains" : "eq" }) });
                      }}
                      spellCheck={false}
                    />
                    {info ? (
                      <span className="sf-hint" title={`${info.kind}${info.set ? ` · values named by ${info.set}` : ""} · in ${info.lists} list${info.lists === 1 ? "" : "s"}`}>
                        <span className={`sf-kind ${info.kind}`}>{info.kind}</span>
                        {info.set && <span className="sf-set truncate">{info.set}</span>}
                      </span>
                    ) : unknownField ? (
                      <span className="sf-hint danger">No list of this file has this field</span>
                    ) : null}
                  </div>
                  <select className="sf-control" value={c.op} onChange={(e) => update(i, { op: e.target.value as SearchOp })}>
                    {ops.map((o) => (
                      <option key={o.op} value={o.op}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                  {needsValue ? (
                    <>
                      <input
                        className="sf-control"
                        list={valueLabels?.length ? `search-labels-${i}` : undefined}
                        placeholder={
                          c.op === "in" || c.op === "not_in"
                            ? "7, 8, 15"
                            : c.op === "has_flags" || c.op === "lacks_flags"
                              ? info?.set
                                ? "labels | joined, or a number"
                                : "e.g. 16 or 0x10"
                              : info?.set
                                ? "number or label"
                                : "value"
                        }
                        value={c.value}
                        onChange={(e) => update(i, { value: e.target.value })}
                        spellCheck={false}
                      />
                      {valueLabels?.length ? (
                        <datalist id={`search-labels-${i}`}>
                          {valueLabels.map((l) => (
                            <option key={l} value={l} />
                          ))}
                        </datalist>
                      ) : null}
                    </>
                  ) : (
                    <span className="sf-novalue muted small">no value needed</span>
                  )}
                  <button
                    className="icon-btn small sf-remove"
                    onClick={() => setConditions((cs) => (cs.length > 1 ? cs.filter((_, j) => j !== i) : [newCondition()]))}
                    title={conditions.length > 1 ? "Remove this condition" : "Clear this condition"}
                    aria-label="Remove condition"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              );
            })}
            <div className="sf-cond-actions">
              <button className="sf-add" onClick={() => setConditions((cs) => [...cs, newCondition()])} disabled={conditions.length >= 10}>
                <Plus size={14} /> Add condition
              </button>
              {conditions.length > 1 && (
                <div className="sf-match">
                  <span className="sf-label">Match</span>
                  <div className="segmented" role="radiogroup" aria-label="Match">
                    <button className={matchAll ? "active" : ""} onClick={() => setMatchAll(true)} title="Records meeting every condition (lists without one of the fields are skipped)">
                      All
                    </button>
                    <button className={!matchAll ? "active" : ""} onClick={() => setMatchAll(false)} title="Records meeting at least one condition">
                      Any
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        ) : (
          <>
            <div className="sf-value">
              <label className="sf-field">
                <span className="sf-label">Type</span>
                <select className="sf-control" value={kind} onChange={(e) => setKind(e.target.value as ValueKind)}>
                  <option value="int">Integer</option>
                  <option value="float">Float</option>
                  <option value="text">Text</option>
                  <option value="hex">Hex bytes</option>
                </select>
              </label>
              <label className="sf-field">
                <span className="sf-label">Value</span>
                <input className="sf-control" autoFocus placeholder={PLACEHOLDER[kind]} value={value} onChange={(e) => setValue(e.target.value)} spellCheck={false} />
              </label>
            </div>
            {(kind === "text" || kind === "int" || kind === "float") && (
              <div className="search-row">
                {kind === "text" && (
                  <label className="check">
                    <input type="checkbox" checked={caseSensitive} onChange={(e) => setCaseSensitive(e.target.checked)} /> Match case
                  </label>
                )}
                {(kind === "int" || kind === "float") && (
                  <label className="check" title="Also look in bytes no layout describes (4-byte aligned)">
                    <input type="checkbox" checked={includeUnknown} onChange={(e) => setIncludeUnknown(e.target.checked)} /> Include undescribed bytes
                  </label>
                )}
              </div>
            )}
          </>
        )}

        <div className="sf-submit">
          <button className="btn primary" onClick={run} disabled={busy}>
            {busy ? <Loader2 size={14} className="spin" /> : <Search size={14} />} Search
          </button>
          <span className="muted small">
            or press <kbd>Enter</kbd>
          </span>
        </div>
        {error && (
          <div className="se-problems">
            <CircleAlert size={13} /> {error.replace(/^Error: /, "")}
          </div>
        )}
      </div>

      {report && (
        <div className="search-summary">
          <span>
            <b>{count(report.matchedRecords)}</b> record{report.matchedRecords === 1 ? "" : "s"} in <b>{report.matchedLists}</b> list
            {report.matchedLists === 1 ? "" : "s"}
            <span className="muted">
              {" "}
              · scanned {count(report.scannedRecords)} in {report.scannedLists} · {report.elapsedMs} ms
            </span>
          </span>
          <span className="spacer" />
          {report.hits.length > 0 && (
            <button className="link" onClick={copyIds} title="Copy the IDs of the results, one per line">
              {copied ? <Check size={12} /> : <Copy size={12} />} Copy IDs
            </button>
          )}
        </div>
      )}
      {report?.truncated && <div className="search-note muted small">Showing the first {report.hits.length}. Narrow the search to see the rest.</div>}

      <div className="search-results scroll">
        {report && report.hits.length === 0 && (
          <div className="empty-note">
            {report.scannedLists === 0 ? "No list of this file has the fields these conditions use." : "Nothing matches."}
          </div>
        )}
        {groups.map(([list, hits]) => {
          const closed = collapsed.has(list);
          return (
            <div key={list} className="search-group">
              <button
                className="search-group-head"
                onClick={() =>
                  setCollapsed((c) => {
                    const next = new Set(c);
                    if (next.has(list)) next.delete(list);
                    else next.add(list);
                    return next;
                  })
                }
              >
                <ChevronRight size={14} className={"caret-icon" + (closed ? "" : " open")} />
                <span className="truncate">{lists[list]?.name ?? `List ${list}`}</span>
                <span className="muted mono small">#{list}</span>
                <span className="spacer" />
                <span className="muted small">{hits.length}</span>
              </button>
              {!closed &&
                hits.map((h) => {
                  const [m, ...more] = h.matches;
                  return (
                    <button
                      key={h.row}
                      className={"search-hit" + (active === `${h.list}:${h.row}` ? " active" : "")}
                      onClick={(e) => open(h, e)}
                      onAuxClick={(e) => e.button === 1 && open(h, e)}
                      title="Open (Ctrl+click: new tab)"
                    >
                      <span className="find-icon">{icon?.(h.icon) ? <img src={icon(h.icon)} alt="" draggable={false} /> : null}</span>
                      <span className="search-hit-main">
                        <span className="truncate">{h.name || <span className="muted">Unnamed</span>}</span>
                        {m && (
                          <span className="search-hit-match truncate mono">
                            {m.field} = {m.value === "" ? '""' : m.value}
                            {m.label && <span className="muted"> ({m.label})</span>}
                            {more.length > 0 && <span className="muted"> +{more.length}</span>}
                          </span>
                        )}
                      </span>
                      <span className="mono muted small">{h.id}</span>
                    </button>
                  );
                })}
            </div>
          );
        })}
      </div>
    </section>
  );
}
