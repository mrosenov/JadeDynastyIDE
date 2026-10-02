import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDownUp,
  Binary,
  CircleAlert,
  ListOrdered,
  Plus,
  RotateCcw,
  Save,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { deleteNamedSet, namedSet, namedSets, saveNamedSet } from "../elements/api";
import type { FileSummary, NamedSet, SetDetail, SetKind, SetOrigin, SetSummary } from "../elements/types";
import { bitHex, slug } from "../elements/bits";

interface Props {
  /** Set to show first. */
  initialKey?: string | null;
  /** Called after a save or delete, with the open file re-read (if any). */
  onChanged: (summary: FileSummary | null) => void;
  onClose: () => void;
}

interface Row {
  /** Value (enums) or bit (masks). */
  n: number;
  label: string;
  description: string;
}

interface Draft {
  kind: SetKind;
  key: string;
  label: string;
  rows: Row[];
  /** Not saved yet: the key can still change. */
  isNew: boolean;
}

const ORIGIN_LABEL: Record<SetOrigin, string> = { builtin: "", override: "edited", user: "yours" };

function toDraft(detail: SetDetail): Draft {
  const rows =
    detail.kind === "mask"
      ? (detail.set.flags ?? []).map((f) => ({ n: f.bit, label: f.label, description: f.description ?? "" }))
      : (detail.set.values ?? []).map((v) => ({ n: v.value, label: v.label, description: v.description ?? "" }));
  return { kind: detail.kind, key: detail.set.key, label: detail.set.label, rows, isNew: false };
}

function toSet(d: Draft): NamedSet {
  const items = d.rows.map((r) => ({ label: r.label.trim(), ...(r.description.trim() ? { description: r.description.trim() } : {}) }));
  return d.kind === "mask"
    ? { key: d.key, label: d.label.trim(), flags: d.rows.map((r, i) => ({ bit: r.n, ...items[i] })) }
    : { key: d.key, label: d.label.trim(), values: d.rows.map((r, i) => ({ value: r.n, ...items[i] })) };
}

function problemsOf(d: Draft, taken: Set<string>): string[] {
  const out: string[] = [];
  if (!/^[a-z0-9_]{1,64}$/.test(d.key)) out.push("The key may only use lowercase letters, digits and _.");
  else if (d.isNew && taken.has(d.key)) out.push(`The key "${d.key}" is already used.`);
  const seen = new Set<number>();
  for (const r of d.rows) {
    const what = d.kind === "mask" ? `Bit ${r.n}` : `Value ${r.n}`;
    if (!Number.isInteger(r.n)) out.push(`${what} is not a whole number.`);
    else if (d.kind === "mask" && (r.n < 0 || r.n > 63)) out.push(`${what} is out of range (0–63).`);
    if (!r.label.trim()) out.push(`${what} needs a label.`);
    if (seen.has(r.n)) out.push(`${what} is listed twice.`);
    seen.add(r.n);
  }
  return [...new Set(out)];
}

/** Editor for the shared enums (named values) and masks (named bits). */
export function SetsEditor({ initialKey, onChanged, onClose }: Props) {
  const [sets, setSets] = useState<SetSummary[] | null>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<string | null>(initialKey ?? null);
  const [detail, setDetail] = useState<SetDetail | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [baseline, setBaseline] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showUsage, setShowUsage] = useState(false);

  useEffect(() => {
    namedSets()
      .then((list) => {
        setSets(list);
        setSelected((s) => s ?? list[0]?.key ?? null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  // Load the selected set (unless it is a new, unsaved one).
  useEffect(() => {
    if (!selected || draft?.isNew) return;
    let cancelled = false;
    namedSet(selected)
      .then((d) => {
        if (cancelled) return;
        const next = toDraft(d);
        setDetail(d);
        setDraft(next);
        setBaseline(JSON.stringify(toSet(next)));
        setShowUsage(false);
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [selected]);

  const dirty = draft !== null && (draft.isNew || JSON.stringify(toSet(draft)) !== baseline);
  const taken = useMemo(() => new Set((sets ?? []).map((s) => s.key)), [sets]);
  const problems = draft ? problemsOf(draft, taken) : [];

  const confirmDiscard = useCallback(
    () => !dirty || window.confirm("Discard your unsaved changes to this set?"),
    [dirty],
  );

  const pick = (key: string) => {
    if (key === selected && !draft?.isNew) return;
    if (!confirmDiscard()) return;
    setDraft(null);
    setSelected(key);
  };

  const create = (kind: SetKind) => {
    if (!confirmDiscard()) return;
    const base = kind === "mask" ? "new_mask" : "new_enum";
    let key = base;
    for (let n = 2; taken.has(key); n++) key = `${base}_${n}`;
    setDetail(null);
    setSelected(null);
    setDraft({ kind, key, label: kind === "mask" ? "New mask" : "New enum", rows: [], isNew: true });
    setShowUsage(false);
  };

  const close = useCallback(() => confirmDiscard() && onClose(), [confirmDiscard, onClose]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !(e.target instanceof HTMLInputElement)) {
        e.stopPropagation();
        close();
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        // Save this set, not whatever is open underneath.
        e.preventDefault();
        e.stopPropagation();
        saveRef.current();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [close]);

  const applyChange = async (run: () => ReturnType<typeof saveNamedSet>, nextKey: string | null) => {
    setBusy(true);
    setError(null);
    try {
      const changed = await run();
      setSets(changed.sets);
      onChanged(changed.summary);
      setDraft(null);
      const stillThere = nextKey && changed.sets.some((s) => s.key === nextKey);
      setSelected(stillThere ? nextKey : (changed.sets[0]?.key ?? null));
      // Re-select even if the key did not change, to reload the saved set.
      if (stillThere && nextKey === selected) {
        const d = await namedSet(nextKey);
        const next = toDraft(d);
        setDetail(d);
        setDraft(next);
        setBaseline(JSON.stringify(toSet(next)));
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const save = () => {
    if (draft && dirty && !busy && !problems.length) applyChange(() => saveNamedSet(draft.kind, toSet(draft)), draft.key);
  };
  const saveRef = useRef(save);
  saveRef.current = save;

  const remove = () => {
    if (!draft || !detail) return;
    const used = detail.usage.length ? ` ${detail.usage.length} field(s) use it and will show raw values.` : "";
    const msg =
      detail.origin === "override"
        ? `Revert "${draft.label}" to its built-in definition?`
        : `Delete "${draft.label}"?${used}`;
    if (window.confirm(msg)) applyChange(() => deleteNamedSet(draft.key), detail.origin === "override" ? draft.key : null);
  };

  const update = (patch: Partial<Draft>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const updateRow = (i: number, patch: Partial<Row>) =>
    setDraft((d) => (d ? { ...d, rows: d.rows.map((r, j) => (j === i ? { ...r, ...patch } : r)) } : d));
  const addRow = () =>
    setDraft((d) => {
      if (!d) return d;
      const used = new Set(d.rows.map((r) => r.n));
      let n = d.kind === "mask" ? 0 : d.rows.length ? Math.max(...d.rows.map((r) => r.n)) + 1 : 0;
      while (used.has(n) && n < 64) n++;
      return { ...d, rows: [...d.rows, { n, label: "", description: "" }] };
    });
  const sortRows = () => setDraft((d) => (d ? { ...d, rows: [...d.rows].sort((a, b) => a.n - b.n) } : d));

  const q = query.trim().toLowerCase();
  const visible = (sets ?? []).filter((s) => !q || s.label.toLowerCase().includes(q) || s.key.includes(q));
  const groups: [SetKind, string][] = [
    ["mask", "Masks"],
    ["enum", "Enums"],
  ];

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal sets-editor" role="dialog" aria-label="Enums and masks">
        <header className="modal-head">
          <ListOrdered size={17} />
          <h3>Enums &amp; masks</h3>
          <span className="muted small">Shared by every layout. Fields pick one by key.</span>
          <span className="spacer" />
          <button className="icon-btn" onClick={close} aria-label="Close">
            <X size={18} />
          </button>
        </header>

        <div className="sets-body">
          <aside className="sets-list">
            <div className="import-search">
              <Search size={14} />
              <input
                className="search"
                placeholder="Filter…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                spellCheck={false}
              />
            </div>
            <div className="sets-new">
              <button className="btn small" onClick={() => create("mask")} title="Named bits of a bit field">
                <Plus size={13} /> Mask
              </button>
              <button className="btn small" onClick={() => create("enum")} title="A name for each value">
                <Plus size={13} /> Enum
              </button>
            </div>
            <div className="scroll">
              {draft?.isNew && (
                <div className="sets-group">
                  <div className="sets-group-title">New</div>
                  <button className="set-item active">
                    {draft.kind === "mask" ? <Binary size={13} /> : <ListOrdered size={13} />}
                    <span className="truncate">{draft.label || draft.key}</span>
                    <span className="set-origin">unsaved</span>
                  </button>
                </div>
              )}
              {groups.map(([kind, title]) => {
                const items = visible.filter((s) => s.kind === kind);
                return (
                  <div className="sets-group" key={kind}>
                    <div className="sets-group-title">
                      {title} <span className="muted">{items.length}</span>
                    </div>
                    {items.map((s) => (
                      <button
                        key={s.key}
                        className={"set-item" + (s.key === selected && !draft?.isNew ? " active" : "")}
                        onClick={() => pick(s.key)}
                        title={`${s.key} · ${s.count} ${kind === "mask" ? "bits" : "values"}`}
                      >
                        <span className="truncate">{s.label}</span>
                        {ORIGIN_LABEL[s.origin] && <span className={`set-origin ${s.origin}`}>{ORIGIN_LABEL[s.origin]}</span>}
                        <span className="muted mono small">{s.count}</span>
                      </button>
                    ))}
                  </div>
                );
              })}
            </div>
          </aside>

          <section className="sets-detail">
            {!draft ? (
              <div className="empty-note center">{sets ? "Pick an enum or mask, or create one." : "Loading…"}</div>
            ) : (
              <>
                <div className="sets-meta">
                  <label>
                    <span>Label</span>
                    <input
                      className="se-input"
                      value={draft.label}
                      onChange={(e) => {
                        const label = e.target.value;
                        // A new set's key follows its label until edited.
                        const follow = draft.isNew && draft.key === slug(draft.label);
                        update({ label, ...(follow && slug(label) ? { key: slug(label) } : {}) });
                      }}
                    />
                  </label>
                  <label>
                    <span>Key</span>
                    <input
                      className={"se-input mono" + (problems.some((p) => p.startsWith("The key")) ? " invalid" : "")}
                      value={draft.key}
                      readOnly={!draft.isNew}
                      title={draft.isNew ? "Fields refer to the set by this key" : "The key of a saved set cannot change: fields refer to it"}
                      onChange={(e) => update({ key: e.target.value })}
                      spellCheck={false}
                    />
                  </label>
                  <span className={`tag ${draft.kind === "mask" ? "warn" : "ok"}`}>{draft.kind}</span>
                </div>

                <div className="se-toolbar">
                  <button className="btn small" onClick={addRow}>
                    <Plus size={14} /> {draft.kind === "mask" ? "Bit" : "Value"}
                  </button>
                  <button className="btn small" onClick={sortRows} disabled={draft.rows.length < 2}>
                    <ArrowDownUp size={14} /> Sort
                  </button>
                  <span className="spacer" />
                  {detail && (
                    <button className="link" onClick={() => setShowUsage((u) => !u)}>
                      Used by {detail.usage.length} field{detail.usage.length === 1 ? "" : "s"}
                    </button>
                  )}
                </div>
                {showUsage && detail && (
                  <div className="sets-usage scroll">
                    {detail.usage.length ? detail.usage.map((u) => <div key={u} className="mono small">{u}</div>) : <span className="muted small">No field uses it yet.</span>}
                  </div>
                )}

                <div className="sets-rows scroll">
                  <div className={"sets-row sets-row-head" + (draft.kind === "mask" ? " mask" : "")}>
                    <span>{draft.kind === "mask" ? "Bit" : "Value"}</span>
                    {draft.kind === "mask" && <span>Value</span>}
                    <span>Label</span>
                    <span>Description</span>
                    <span />
                  </div>
                  {draft.rows.length === 0 && (
                    <div className="empty-note">
                      No {draft.kind === "mask" ? "bits" : "values"} yet. Add one with the button above.
                    </div>
                  )}
                  {draft.rows.map((r, i) => (
                    <div className={"sets-row" + (draft.kind === "mask" ? " mask" : "")} key={i}>
                      <input
                        className="cell mono num"
                        type="number"
                        min={draft.kind === "mask" ? 0 : undefined}
                        max={draft.kind === "mask" ? 63 : undefined}
                        value={Number.isFinite(r.n) ? r.n : ""}
                        onChange={(e) => updateRow(i, { n: e.target.value === "" ? NaN : Math.trunc(Number(e.target.value)) })}
                      />
                      {draft.kind === "mask" && (
                        <span className="mono muted small">{r.n >= 0 && r.n <= 63 ? bitHex(r.n) : "—"}</span>
                      )}
                      <input
                        className={"cell" + (r.label.trim() ? "" : " invalid")}
                        value={r.label}
                        placeholder="Label"
                        onChange={(e) => updateRow(i, { label: e.target.value })}
                      />
                      <input
                        className="cell"
                        value={r.description}
                        placeholder="Description (optional)"
                        onChange={(e) => updateRow(i, { description: e.target.value })}
                      />
                      <span className="se-actions">
                        <button
                          title="Remove"
                          className="danger"
                          onClick={() => setDraft((d) => (d ? { ...d, rows: d.rows.filter((_, j) => j !== i) } : d))}
                        >
                          <Trash2 size={14} />
                        </button>
                      </span>
                    </div>
                  ))}
                </div>

                {(problems.length > 0 || error) && (
                  <div className="se-problems">
                    <CircleAlert size={13} /> {error ?? problems.slice(0, 3).join(" ")}
                  </div>
                )}

                <footer className="modal-foot">
                  {detail && detail.origin !== "builtin" && (
                    <button className="btn" onClick={remove} disabled={busy}>
                      {detail.origin === "override" ? (
                        <>
                          <RotateCcw size={14} /> Revert to built-in
                        </>
                      ) : (
                        <>
                          <Trash2 size={14} /> Delete
                        </>
                      )}
                    </button>
                  )}
                  <span className="muted small">
                    {draft.isNew
                      ? "New sets are saved to your user folder."
                      : detail?.origin === "builtin"
                        ? "Saving keeps your changes in your user folder; the built-in stays as it is."
                        : "Saved in your user folder."}
                  </span>
                  <span className="spacer" />
                  <button
                    className="btn"
                    disabled={!dirty || busy}
                    onClick={() => {
                      if (draft.isNew) {
                        setDraft(null);
                        setSelected(sets?.[0]?.key ?? null);
                      } else if (detail) {
                        const next = toDraft(detail);
                        setDraft(next);
                      }
                    }}
                  >
                    Discard
                  </button>
                  <button className="btn primary" onClick={save} disabled={!dirty || busy || problems.length > 0}>
                    <Save size={14} /> {busy ? "Saving…" : "Save"}
                  </button>
                </footer>
              </>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
