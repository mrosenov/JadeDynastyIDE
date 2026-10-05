import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowLeft, BrainCircuit, CircleAlert, FileClock, FileSearch, FolderOpen, Loader2, LockKeyhole, ShieldCheck, Sparkles, X } from "lucide-react";
import { analyzeListFromReference, analyzeListLayout, importCandidates } from "../elements/api";
import type { ImportCandidate, LayoutAnalysis } from "../elements/types";

interface Props {
  list: number;
  listName: string;
  itemSize: number;
  targetVersion: number;
  aiConfigured: boolean;
  suggestedReference: string;
  onApply: (analysis: LayoutAnalysis) => void;
  onClose: () => void;
}

type Method = "ai" | "schema";
const STORAGE_KEY = "jdide.layoutAnalysisReference";

export function LayoutAnalyzerDialog({ list, listName, itemSize, targetVersion, aiConfigured, suggestedReference, onApply, onClose }: Props) {
  const [method, setMethod] = useState<Method | null>(null);
  const [reference, setReference] = useState(() => localStorage.getItem(STORAGE_KEY) || suggestedReference);
  const [candidates, setCandidates] = useState<ImportCandidate[]>([]);
  const [sourceLayout, setSourceLayout] = useState("");
  const [loadingCandidates, setLoadingCandidates] = useState(true);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<LayoutAnalysis | null>(null);

  const older = useMemo(
    () => candidates.filter((candidate) => candidate.version < targetVersion).sort((a, b) => b.version - a.version || a.layoutId.localeCompare(b.layoutId)),
    [candidates, targetVersion],
  );

  useEffect(() => {
    let cancelled = false;
    setLoadingCandidates(true);
    importCandidates(list)
      .then((items) => {
        if (cancelled) return;
        setCandidates(items);
        const first = items.filter((candidate) => candidate.version < targetVersion).sort((a, b) => b.version - a.version)[0];
        setSourceLayout(first?.layoutId ?? "");
      })
      .catch((e) => !cancelled && setError(String(e)))
      .finally(() => !cancelled && setLoadingCandidates(false));
    return () => {
      cancelled = true;
    };
  }, [list, targetVersion]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !running) {
        event.stopImmediatePropagation();
        if (result) setResult(null);
        else if (method) setMethod(null);
        else onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [method, onClose, result, running]);

  const browse = async () => {
    const picked = await open({ multiple: false, directory: false, title: "Pick the elements.data matching the older schema", filters: [{ name: "elements.data", extensions: ["data"] }] });
    if (typeof picked === "string") {
      setReference(picked);
      setResult(null);
      setError(null);
    }
  };

  const analyze = async () => {
    const path = reference.trim();
    if (!path || !method || !sourceLayout) return;
    setRunning(true);
    setError(null);
    setResult(null);
    try {
      localStorage.setItem(STORAGE_KEY, path);
      setResult(method === "ai" ? await analyzeListLayout(path, sourceLayout, list) : await analyzeListFromReference(path, sourceLayout, list));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  const choose = (next: Method) => {
    setMethod(next);
    setError(null);
    setResult(null);
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !running && onClose()}>
      <div className="modal layout-analyzer-dialog" role="dialog" aria-label={`Analyze ${listName} layout`}>
        <header className="modal-head">
          <BrainCircuit size={18} />
          <div>
            <h3>Analyze list layout</h3>
            <p className="muted small">List {list} · {listName} · {itemSize} bytes per record</p>
          </div>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} disabled={running} aria-label="Close"><X size={18} /></button>
        </header>

        <div className="modal-body scroll">
          {!method ? (
            <div className="analyzer-methods">
              <button className="analyzer-method" onClick={() => choose("schema")} disabled={loadingCandidates || older.length === 0}>
                <FileClock size={22} />
                <span><b>Analyze from an older schema</b><small>Local analysis · no API usage</small></span>
                <p>Choose an older layout and its matching elements.data. Known fields are relocated from same-ID records; new byte spans are marked for you to define.</p>
              </button>
              <button className="analyzer-method" onClick={() => choose("ai")} disabled={!aiConfigured || loadingCandidates || older.length === 0}>
                {aiConfigured ? <Sparkles size={22} /> : <LockKeyhole size={22} />}
                <span><b>Analyze with AI</b><small>{aiConfigured ? "Uses the configured endpoint" : "Configure endpoint, model and API key in Settings"}</small></span>
                <p>Build the same paired evidence, then ask the configured model to infer names and types for fields added in the newer version.</p>
              </button>
              {loadingCandidates && <div className="analyzer-running compact"><Loader2 className="spin" size={16} /> Loading older schemas…</div>}
              {!loadingCandidates && older.length === 0 && <div className="settings-status bad"><CircleAlert size={14} /> No older schema is available for this list.</div>}
              {error && <div className="settings-status bad"><CircleAlert size={14} /> <span>{error}</span></div>}
            </div>
          ) : !result ? (
            <>
              <section className="analyzer-intro">
                {method === "ai" ? <Sparkles size={18} /> : <FileClock size={18} />}
                <div>
                  <b>{method === "ai" ? "AI analysis with a trusted older file" : "Local alignment from an older schema"}</b>
                  <p className="muted small">
                    {method === "ai"
                      ? "JD IDE sends bounded evidence for this list to the configured endpoint. The exact target schema is hidden from the model."
                      : "JD IDE compares same-ID records locally. It keeps proven older fields and marks newly inserted or uncertain bytes as unknown."}
                  </p>
                </div>
              </section>
              <label className="settings-field">
                <span>Older schema</span>
                <select className="se-input" value={sourceLayout} onChange={(event) => setSourceLayout(event.target.value)} disabled={running}>
                  {older.map((candidate) => (
                    <option value={candidate.layoutId} key={candidate.layoutId}>v{candidate.version} · {candidate.layoutId} · {candidate.name} · {candidate.size} B</option>
                  ))}
                </select>
              </label>
              <label className="settings-field">
                <span>Matching older elements.data</span>
                <span className="settings-row">
                  <input className="se-input mono" value={reference} onChange={(event) => setReference(event.target.value)} placeholder="Path to the elements.data for this schema" spellCheck={false} disabled={running} />
                  <button className="btn" onClick={browse} disabled={running}><FolderOpen size={15} /> Browse…</button>
                </span>
              </label>
              <div className="analyzer-safety">
                <ShieldCheck size={16} />
                <span>The selected schema and reference file must match. The result opens as an unsaved draft; record bytes are never changed.</span>
              </div>
              {running && <div className="analyzer-running"><Loader2 className="spin" size={20} /><div><b>{method === "ai" ? "Comparing records and asking the model…" : "Matching records and relocating fields…"}</b>{method === "ai" && <span className="muted small">This can take up to two minutes.</span>}</div></div>}
              {error && <div className="settings-status bad"><CircleAlert size={14} /> <span>{error}</span></div>}
            </>
          ) : (
            <div className="analyzer-result">
              <div className="analyzer-score">
                <span>{result.confidence}%</span>
                <div><b>{method === "ai" ? "AI proposal ready" : "Older schema proposal ready"}</b><small className="muted">{result.matchedRecords} paired records · reference list {result.referenceList}</small></div>
              </div>
              <p>{result.summary}</p>
              <div className="analyzer-result-meta">
                <span><b>{result.definition.fields?.length ?? 0}</b> top-level fields</span>
                <span><b>{result.definition.size}</b> bytes</span>
                <span className="mono">{result.definition.struct || result.definition.name}</span>
              </div>
              {result.warnings.length > 0 && <section className="analyzer-warnings"><h4><CircleAlert size={15} /> Review these uncertainties</h4><ul>{result.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul></section>}
              <div className="analyzer-safety"><FileSearch size={16} /><span>Use proposal loads these fields into the normal schema editor. Review the record preview before saving.</span></div>
            </div>
          )}
        </div>

        <footer className="modal-foot">
          <button className="btn" onClick={onClose} disabled={running}>Cancel</button>
          <span className="spacer" />
          {method && !result && <button className="btn" onClick={() => setMethod(null)} disabled={running}><ArrowLeft size={14} /> Methods</button>}
          {method && !result && <button className="btn primary" onClick={analyze} disabled={running || !reference.trim() || !sourceLayout}>{running ? <Loader2 className="spin" size={15} /> : method === "ai" ? <Sparkles size={15} /> : <FileClock size={15} />} {running ? "Analyzing…" : "Analyze"}</button>}
          {result && <><button className="btn" onClick={() => setResult(null)}>Analyze again</button><button className="btn primary" onClick={() => onApply(result)}>Use proposal</button></>}
        </footer>
      </div>
    </div>
  );
}
