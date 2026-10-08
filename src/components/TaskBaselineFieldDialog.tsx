import { useEffect, useMemo, useState } from "react";
import { Check, Loader2, Pencil, Trash2, X } from "lucide-react";
import { getTaskSchema } from "../elements/api";
import type { TaskSchemaView } from "../elements/types";

export interface TaskBaselineFieldDraft {
  mode: "remove" | "replace";
  structure: string;
  field: string;
  fieldType: string;
}

interface Props {
  version: number;
  baselineVersion: number;
  onApply: (draft: TaskBaselineFieldDraft) => Promise<boolean>;
  onClose: () => void;
}

const FIELD_TYPES = ["bool8", "uint8", "int8", "uint16", "int16", "uint32", "int32", "float32", "uint64", "int64", "float64", "bytes[1]", "bytes[2]", "bytes[4]", "bytes[8]", "bytes[16]", "bytes[32]", "raw8", "raw16", "raw32", "raw64"];
const typeWidth = (type: string) => type === "bool8" || type.endsWith("8") && !type.endsWith("64") ? 1 : type.endsWith("16") ? 2 : type.endsWith("32") ? 4 : type.endsWith("64") ? 8 : Number(type.match(/\[(\d+)\]/)?.[1] ?? 0);

export function TaskBaselineFieldDialog({ version, baselineVersion, onApply, onClose }: Props) {
  const [schema, setSchema] = useState<TaskSchemaView | null>(null);
  const [mode, setMode] = useState<TaskBaselineFieldDraft["mode"]>("replace");
  const [structureName, setStructureName] = useState("");
  const [fieldName, setFieldName] = useState("");
  const [fieldType, setFieldType] = useState("uint32");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getTaskSchema(version, baselineVersion).then((result) => {
      if (cancelled) return;
      setSchema(result);
      setStructureName(result.root);
    }).catch((problem) => !cancelled && setError(String(problem).replace(/^Error: /, "")));
    return () => { cancelled = true; };
  }, [baselineVersion, version]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || busy) return;
      event.stopImmediatePropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy, onClose]);

  const structure = schema?.structures.find((candidate) => candidate.name === structureName) ?? null;
  const fields = useMemo(() => structure?.fields.filter((field) => !field.patched && (mode === "remove" || field.fixedWidth !== undefined)) ?? [], [mode, structure]);
  const field = fields.find((candidate) => candidate.name === fieldName) ?? null;
  const typeOptions = FIELD_TYPES.includes(fieldType) ? FIELD_TYPES : [fieldType, ...FIELD_TYPES];
  const valid = !!structure && !!field;

  const selectMode = (next: TaskBaselineFieldDraft["mode"]) => {
    setMode(next);
    setFieldName("");
  };

  const selectField = (name: string) => {
    setFieldName(name);
    const selected = fields.find((candidate) => candidate.name === name);
    if (selected) setFieldType(selected.fieldType);
  };

  const apply = async () => {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (await onApply({ mode, structure: structureName, field: fieldName, fieldType })) onClose();
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };

  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
    <div className="modal task-baseline-dialog" role="dialog" aria-label={`Change a tasks.data v${version} baseline field`}>
      <header className="modal-head">{mode === "replace" ? <Pencil size={18}/> : <Trash2 size={18}/>}<div><h3>Change baseline field</h3><p className="muted small">tasks.data v{version} · inherited from v{schema?.baselineVersion ?? baselineVersion}</p></div><span className="spacer"/><span className="tag warn">structural patch</span><button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18}/></button></header>
      {!schema && !error ? <div className="task-schema-loading"><Loader2 className="spin" size={18}/> Reading effective schema…</div> : schema && <div className="task-baseline-form">
        <div className="segmented task-baseline-modes" role="radiogroup" aria-label="Baseline field operation"><button className={mode === "replace" ? "active" : ""} onClick={() => selectMode("replace")} role="radio" aria-checked={mode === "replace"}><Pencil size={13}/> Replace type</button><button className={mode === "remove" ? "active" : ""} onClick={() => selectMode("remove")} role="radio" aria-checked={mode === "remove"}><Trash2 size={13}/> Remove field</button></div>
        <label><span>Structure</span><select value={structureName} onChange={(event) => { setStructureName(event.target.value); setFieldName(""); }}>{schema.structures.map((candidate) => <option value={candidate.name} key={candidate.name}>{candidate.name}{candidate.root ? " · root" : ""}</option>)}</select></label>
        <label><span>Inherited field</span><select value={fieldName} onChange={(event) => selectField(event.target.value)}><option value="">Choose a field…</option>{fields.map((candidate) => <option value={candidate.name} key={candidate.name}>{candidate.name} · {candidate.fieldType}</option>)}</select><small>{mode === "replace" ? `${fields.length} fixed-width baseline field${fields.length === 1 ? "" : "s"} available` : `${fields.length} baseline field${fields.length === 1 ? "" : "s"} available`}</small></label>
        {mode === "replace" && <label><span>New type</span><select value={fieldType} onChange={(event) => setFieldType(event.target.value)}>{typeOptions.map((type) => <option value={type} key={type}>{type} · {typeWidth(type)} B</option>)}</select>{field && <small>Current: {field.fieldType} · {field.fixedWidth} B. Changing width moves every following field.</small>}</label>}
        <div className="task-array-explanation"><b>{mode === "replace" ? "The replacement keeps the field name and conditions." : "Dependent fields make removal invalid."}</b><span>{mode === "replace" ? "Count and condition dependencies are checked against the new type." : "JD IDE rejects removal when a later condition, count or insertion still refers to this field."} Every task root is analyzed before the operation is saved.</span></div>
      </div>}
      {error && <div className="path-data-message error" role="alert">{error}</div>}
      <footer className="modal-foot"><span className="muted small">This changes the layout only; task bytes remain untouched.</span><span className="spacer"/><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn primary" onClick={() => void apply()} disabled={!valid || busy}>{busy ? <Loader2 size={14} className="spin"/> : <Check size={14}/>} {busy ? "Analyzing every root…" : mode === "replace" ? "Replace and analyze" : "Remove and analyze"}</button></footer>
    </div>
  </div>;
}
