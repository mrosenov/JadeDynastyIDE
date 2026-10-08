import { useEffect, useMemo, useState } from "react";
import { Braces, Check, Loader2, X } from "lucide-react";
import { getTaskSchema } from "../elements/api";
import type { TaskSchemaView } from "../elements/types";

export interface TaskCountedArrayDraft {
  structure: string;
  afterField: string;
  name: string;
  countField: string;
  itemType: string;
}

interface Props {
  version: number;
  baselineVersion: number;
  operationNumber: number;
  onApply: (draft: TaskCountedArrayDraft) => Promise<boolean>;
  onClose: () => void;
}

const SCALAR_ITEMS = ["uint8", "int8", "bool8", "uint16", "int16", "uint32", "int32", "float32", "uint64", "int64", "float64"];

export function TaskCountedArrayDialog({ version, baselineVersion, operationNumber, onApply, onClose }: Props) {
  const [schema, setSchema] = useState<TaskSchemaView | null>(null);
  const [structureName, setStructureName] = useState("");
  const [afterField, setAfterField] = useState("");
  const [name, setName] = useState(`unknown_v${version}_array_${operationNumber}`);
  const [countField, setCountField] = useState("");
  const [itemType, setItemType] = useState("uint32");
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
  const anchorIndex = structure?.fields.findIndex((field) => field.name === afterField) ?? -1;
  const countFields = useMemo(() => anchorIndex < 0 ? [] : structure?.fields.slice(0, anchorIndex + 1).filter((field) => field.integer).map((field) => field.name) ?? [], [anchorIndex, structure]);
  const valid = !!structure && !!afterField && !!name.trim() && !!countField.trim() && !!itemType;

  const apply = async () => {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (await onApply({ structure: structureName, afterField, name: name.trim(), countField: countField.trim(), itemType })) onClose();
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };

  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
    <div className="modal task-array-dialog" role="dialog" aria-label={`Add a counted array to tasks.data v${version}`}>
      <header className="modal-head"><Braces size={18}/><div><h3>Add counted array</h3><p className="muted small">tasks.data v{version} · based on v{schema?.baselineVersion ?? baselineVersion}</p></div><span className="spacer"/><span className="tag warn">structural patch</span><button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18}/></button></header>
      {!schema && !error ? <div className="task-schema-loading"><Loader2 className="spin" size={18}/> Reading effective schema…</div> : schema && <div className="task-array-form">
        <label><span>Structure</span><select value={structureName} onChange={(event) => { setStructureName(event.target.value); setAfterField(""); setCountField(""); }}>{schema.structures.map((candidate) => <option value={candidate.name} key={candidate.name}>{candidate.name}{candidate.root ? " · root" : ""}</option>)}</select></label>
        <label><span>Insert after</span><select value={afterField} onChange={(event) => { setAfterField(event.target.value); setCountField(""); }}><option value="">Choose a field…</option>{structure?.fields.map((field) => <option value={field.name} key={field.name}>{field.name} · {field.fieldType}</option>)}</select></label>
        <label><span>New field name</span><input value={name} onChange={(event) => setName(event.target.value)} placeholder="unknown_array"/></label>
        <label><span>Count field</span><input value={countField} onChange={(event) => setCountField(event.target.value)} list="task-array-count-fields" placeholder={afterField ? "Earlier integer field" : "Choose an insertion point first"} disabled={!afterField}/><datalist id="task-array-count-fields">{countFields.map((field) => <option value={field} key={field}/>)}</datalist><small>{afterField ? countFields.length ? `${countFields.length} earlier integer field${countFields.length === 1 ? "" : "s"} suggested` : "No earlier integer field is available at this position" : "The count must already be decoded before the array"}</small></label>
        <label><span>Array item</span><select value={itemType} onChange={(event) => setItemType(event.target.value)}><optgroup label="Scalar values">{SCALAR_ITEMS.map((type) => <option value={type} key={type}>{type}</option>)}</optgroup><optgroup label="Existing structures">{schema.structures.map((candidate) => <option value={`struct:${candidate.name}`} key={candidate.name}>{candidate.name}</option>)}</optgroup></select></label>
        <div className="task-array-explanation"><b>The count controls the binary size.</b><span>JD IDE will reject a missing, later or non-integer count field. It validates this schema and rechecks every task root before saving the user patch.</span></div>
      </div>}
      {error && <div className="path-data-message error" role="alert">{error}</div>}
      <footer className="modal-foot"><span className="muted small">This changes the layout only; task bytes remain untouched.</span><span className="spacer"/><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn primary" onClick={() => void apply()} disabled={!valid || busy}>{busy ? <Loader2 size={14} className="spin"/> : <Check size={14}/>} {busy ? "Analyzing every root…" : "Add and analyze"}</button></footer>
    </div>
  </div>;
}
