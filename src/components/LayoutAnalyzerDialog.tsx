import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { BrainCircuit, CircleAlert, FileSearch, FolderOpen, Loader2, ShieldCheck, Sparkles, X } from "lucide-react";
import { analyzeListLayout } from "../elements/api";
import type { LayoutAnalysis } from "../elements/types";

interface Props {
  list: number;
  listName: string;
  itemSize: number;
  suggestedReference: string;
  onApply: (analysis: LayoutAnalysis) => void;
  onClose: () => void;
}

const STORAGE_KEY = "jdide.layoutAnalysisReference";

export function LayoutAnalyzerDialog({ list, listName, itemSize, suggestedReference, onApply, onClose }: Props) {
  const [reference, setReference] = useState(() => localStorage.getItem(STORAGE_KEY) || suggestedReference);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<LayoutAnalysis | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !running) {
        event.stopImmediatePropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose, running]);

  const browse = async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      title: "Pick a trusted elements.data reference",
      filters: [{ name: "elements.data", extensions: ["data"] }],
    });
    if (typeof picked === "string") {
      setReference(picked);
      setResult(null);
      setError(null);
    }
  };

  const analyze = async () => {
    const path = reference.trim();
    if (!path) return;
    setRunning(true);
    setError(null);
    setResult(null);
    try {
      localStorage.setItem(STORAGE_KEY, path);
      setResult(await analyzeListLayout(path, list));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
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
          {!result ? (
            <>
              <section className="analyzer-intro">
                <Sparkles size={18} />
                <div>
                  <b>Use a file with a layout you trust completely</b>
                  <p className="muted small">JD IDE matches records by ID, locates known fields in the newer records, and sends only bounded evidence for this list to the configured AI endpoint.</p>
                </div>
              </section>
              <label className="settings-field">
                <span>Reference elements.data</span>
                <span className="settings-row">
                  <input className="se-input mono" value={reference} onChange={(event) => setReference(event.target.value)} placeholder="Path to the trusted elements.data" spellCheck={false} disabled={running} />
                  <button className="btn" onClick={browse} disabled={running}><FolderOpen size={15} /> Browse…</button>
                </span>
              </label>
              <div className="analyzer-safety">
                <ShieldCheck size={16} />
                <span>The result is checked against the {itemSize}-byte record and opened as an unsaved schema draft. Record bytes are never changed.</span>
              </div>
              {running && <div className="analyzer-running"><Loader2 className="spin" size={20} /><div><b>Comparing records and analyzing fields…</b><span className="muted small">This can take up to two minutes.</span></div></div>}
              {error && <div className="settings-status bad"><CircleAlert size={14} /> <span>{error}</span></div>}
            </>
          ) : (
            <div className="analyzer-result">
              <div className="analyzer-score">
                <span>{result.confidence}%</span>
                <div><b>Proposal ready</b><small className="muted">{result.matchedRecords} paired records · reference list {result.referenceList}</small></div>
              </div>
              <p>{result.summary}</p>
              <div className="analyzer-result-meta">
                <span><b>{result.definition.fields?.length ?? 0}</b> top-level fields</span>
                <span><b>{result.definition.size}</b> bytes</span>
                <span className="mono">{result.definition.struct || result.definition.name}</span>
              </div>
              {result.warnings.length > 0 && (
                <section className="analyzer-warnings">
                  <h4><CircleAlert size={15} /> Review these uncertainties</h4>
                  <ul>{result.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul>
                </section>
              )}
              <div className="analyzer-safety"><FileSearch size={16} /><span>Use proposal loads these fields into the normal schema editor. Review the record preview before saving.</span></div>
            </div>
          )}
        </div>

        <footer className="modal-foot">
          <button className="btn" onClick={onClose} disabled={running}>Cancel</button>
          <span className="spacer" />
          {!result ? (
            <button className="btn primary" onClick={analyze} disabled={running || !reference.trim()}>
              {running ? <Loader2 className="spin" size={15} /> : <Sparkles size={15} />} {running ? "Analyzing…" : "Analyze"}
            </button>
          ) : (
            <>
              <button className="btn" onClick={() => setResult(null)}>Analyze again</button>
              <button className="btn primary" onClick={() => onApply(result)}>Use proposal</button>
            </>
          )}
        </footer>
      </div>
    </div>
  );
}
