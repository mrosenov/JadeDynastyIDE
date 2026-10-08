import { useEffect, useMemo, useState } from "react";
import { Braces, Loader2, Search, ShieldCheck, X } from "lucide-react";
import { getTaskSchema } from "../elements/api";
import type { TaskSchemaView } from "../elements/types";

interface Props {
  version: number;
  baselineVersion: number;
  onClose: () => void;
}

const sourceLabel = (source: TaskSchemaView["source"]) => source === "built_in" ? "Verified built-in" : source === "user_patch" ? "Baseline + user patch" : "Baseline preview";

export function TaskSchemaDialog({ version, baselineVersion, onClose }: Props) {
  const [schema, setSchema] = useState<TaskSchemaView | null>(null);
  const [selected, setSelected] = useState("");
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setSchema(null);
    setError(null);
    getTaskSchema(version, baselineVersion).then((result) => {
      if (cancelled) return;
      setSchema(result);
      setSelected(result.root);
    }).catch((problem) => !cancelled && setError(String(problem).replace(/^Error: /, "")));
    return () => { cancelled = true; };
  }, [baselineVersion, version]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopImmediatePropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const needle = query.trim().toLocaleLowerCase();
  const structures = useMemo(() => schema?.structures.filter((structure) => !needle || structure.name.toLocaleLowerCase().includes(needle) || structure.fields.some((field) => field.name.toLocaleLowerCase().includes(needle) || field.fieldType.toLocaleLowerCase().includes(needle) || field.conditions.some((condition) => condition.toLocaleLowerCase().includes(needle)))) ?? [], [needle, schema]);
  const structure = structures.find((candidate) => candidate.name === selected) ?? structures[0] ?? null;
  const fields = structure?.fields.filter((field) => !needle || structure.name.toLocaleLowerCase().includes(needle) || field.name.toLocaleLowerCase().includes(needle) || field.fieldType.toLocaleLowerCase().includes(needle) || field.conditions.some((condition) => condition.toLocaleLowerCase().includes(needle))) ?? [];
  const fieldCount = schema?.structures.reduce((total, item) => total + item.fields.length, 0) ?? 0;

  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
    <div className="modal task-schema-dialog" role="dialog" aria-label={`tasks.data v${version} schema`}>
      <header className="modal-head">
        <Braces size={18}/>
        <div><h3>Task layout schema</h3><p className="muted small">tasks.data v{version}{schema ? ` · based on v${schema.baselineVersion}` : ""}</p></div>
        <span className="spacer"/>
        {schema && <span className={`tag ${schema.source === "user_patch" ? "warn" : ""}`}>{sourceLabel(schema.source)}</span>}
        <button className="icon-btn" onClick={onClose} aria-label="Close"><X size={18}/></button>
      </header>
      <div className="task-schema-search"><Search size={14}/><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search structures, fields, types or conditions…" autoFocus/></div>
      {!schema && !error ? <div className="task-schema-loading"><Loader2 className="spin" size={19}/> Reading effective schema…</div>
        : error ? <div className="path-data-message error" role="alert">{error}</div>
        : schema && <div className="task-schema-body">
          <aside className="task-schema-structures">
            {structures.map((item) => <button className={(structure?.name === item.name ? "selected" : "") + (item.root ? " root" : "")} onClick={() => setSelected(item.name)} key={item.name}><span className="mono truncate">{item.name}</span><small>{item.fields.length}</small>{item.root && <span className="tag">root</span>}</button>)}
            {!structures.length && <div className="empty-note">No structures match.</div>}
          </aside>
          <section className="task-schema-fields">
            {structure && <><header><div><b className="mono">{structure.name}</b><span>{fields.length} of {structure.fields.length} fields</span></div>{structure.root && <span className="tag">root structure</span>}</header>
              <div className="task-schema-field-head"><span>Field</span><span>Type</span><span>Condition</span><span>Source</span></div>
              <div className="task-schema-field-list">{fields.map((field, index) => <div className={field.patched ? "patched" : ""} key={`${field.name}:${index}`}><span className="mono">{field.name}</span><span className="mono">{field.fieldType}</span><span>{field.conditions.length ? field.conditions.join(" · ") : "Always"}</span><span>{field.patched ? <span className="tag warn">user patch</span> : "baseline"}</span></div>)}{!fields.length && <div className="empty-note">No fields match.</div>}</div>
            </>}
          </section>
        </div>}
      <footer className="modal-foot"><ShieldCheck size={14}/><span className="muted small">Verified schemas are read-only. Unsupported-version changes are made through the analyzer patch controls and rechecked against every root.</span><span className="spacer"/><span className="muted small">{schema ? `${schema.structures.length} structures · ${fieldCount.toLocaleString()} fields` : ""}</span><button className="btn" onClick={onClose}>Close</button></footer>
    </div>
  </div>;
}
