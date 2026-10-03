import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Check, CircleAlert, Download, FolderOpen, Loader2, X } from "lucide-react";
import { exportRecords } from "../elements/api";
import type { ExportSource } from "../elements/types";
import { count } from "../elements/format";

interface FormProps {
  source: ExportSource;
  /** Suggested file name, without extension. */
  name: string;
}

const safeName = (s: string) => s.replace(/[\\/:*?"<>|]+/g, "_").trim() || "export";

/** The export options: a format, labels or not, then a file. */
function ExportForm({ source, name }: FormProps) {
  const [format, setFormat] = useState<"csv" | "json">("csv");
  const [labels, setLabels] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<{ path: string; records: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setError(null);
    setDone(null);
    const path = await save({
      defaultPath: `${safeName(name)}.${format}`,
      filters: [format === "csv" ? { name: "CSV", extensions: ["csv"] } : { name: "JSON", extensions: ["json"] }],
    });
    if (!path) return;
    setBusy(true);
    try {
      const r = await exportRecords(source, format, labels, path);
      setDone({ path: r.path, records: r.records });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="sf-label">Format</div>
      <div className="segmented export-format">
        <button className={format === "csv" ? "active" : ""} onClick={() => setFormat("csv")} title="One row per record; opens in Excel, LibreOffice or Google Sheets">
          CSV
        </button>
        <button className={format === "json" ? "active" : ""} onClick={() => setFormat("json")} title="One object per record; numbers stay numbers">
          JSON
        </button>
      </div>
      <label className="check" title="Adds a column with the label next to each enum or mask field (field#label)">
        <input type="checkbox" checked={labels} onChange={(e) => setLabels(e.target.checked)} /> Add enum and mask labels
      </label>
      <p className="muted small export-note">
        Every field gets a column (<span className="mono">addons[2].id</span>). <span className="mono">_list</span> and{" "}
        <span className="mono">_row</span> say where each record came from.
      </p>
      <button className="btn primary small" onClick={run} disabled={busy}>
        {busy ? <Loader2 size={13} className="spin" /> : <Download size={13} />} Save as…
      </button>
      {done && (
        <div className="export-done small">
          <Check size={13} /> {count(done.records)} record{done.records === 1 ? "" : "s"} exported.
          <button className="link" onClick={() => revealItemInDir(done.path)}>
            <FolderOpen size={12} /> Show
          </button>
        </div>
      )}
      {error && (
        <div className="se-problems">
          <CircleAlert size={13} /> {error}
        </div>
      )}
    </>
  );
}

/** An "Export" link with the options in a popover (e.g. for search results). */
export function ExportMenu({ source, name, label = "Export" }: FormProps & { label?: string }) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => !box.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  return (
    <div className="export-menu" ref={box}>
      <button className={"link" + (open ? " active" : "")} onClick={() => setOpen((o) => !o)} title="Export these records as CSV or JSON">
        <Download size={13} /> {label}
      </button>
      {open && (
        <div className="export-pop" role="dialog" aria-label="Export">
          <ExportForm source={source} name={name} />
        </div>
      )}
    </div>
  );
}

/** The export options in a dialog (File › Export). */
export function ExportDialog({ source, name, title, onClose }: FormProps & { title: string; onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal export-dialog" role="dialog" aria-label={title}>
        <header className="modal-head">
          <Download size={16} />
          <h3 className="truncate">{title}</h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </header>
        <div className="export-dialog-body">
          <ExportForm source={source} name={name} />
        </div>
      </div>
    </div>
  );
}
