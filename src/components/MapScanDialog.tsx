import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Check, CircleAlert, FolderOpen, Loader2, RefreshCw, TriangleAlert, X } from "lucide-react";
import { scanMapRegions, syncMapServer } from "../elements/api";
import { count } from "../elements/format";
import type { ClientMap, MapScanRow } from "../elements/types";

interface Props {
  clientRoot: string | null;
  serverRoot: string | null;
  maps: ClientMap[];
  backup: boolean;
  onServerRoot: (path: string) => void;
  /** Opens a client file in the editor. */
  onOpen: (path: string) => void;
  onClose: () => void;
}

type Filter = "attention" | "missing" | "all";

const status = (row: MapScanRow) => row.clientError ? "unreadable" : row.serverError === "missing" ? "no server file" : row.serverError ? "server unreadable" : row.differences.length ? "differs" : row.errors ? "problems" : "in step";

/** Every map of a client `maps` folder against the server's `config` folder: unreadable files, server files out of
 * step (timestamp, portals, values) and problems; server files can be written from the client files. */
export function MapScanDialog({ clientRoot: initialClient, serverRoot: initialServer, maps, backup, onServerRoot, onOpen, onClose }: Props) {
  const [clientRoot, setClientRoot] = useState(initialClient ?? "");
  const [serverRoot, setServerRoot] = useState(initialServer ?? "");
  const [rows, setRows] = useState<MapScanRow[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("attention");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const names = useMemo(() => {
    const out = new Map<string, string>();
    for (const map of maps) { const key = map.path.toLowerCase(); out.set(key, out.has(key) ? `${out.get(key)}, ${map.name}` : map.name); }
    return out;
  }, [maps]);
  const aliases = useMemo(() => {
    const out: Record<string, string[]> = {};
    for (const map of maps) if (map.dataPath && map.dataPath.toLowerCase() !== map.path.toLowerCase()) (out[map.path.toLowerCase()] ??= []).push(map.dataPath);
    return out;
  }, [maps]);

  const scan = async (client = clientRoot, server = serverRoot) => {
    if (!client) return;
    setBusy(true);
    setError(null);
    try {
      const found = await scanMapRegions(client, server || null, aliases, maps.map((map) => map.id));
      setRows(found);
      setPicked(new Set(found.filter((row) => row.canSync).map((row) => row.clientPath!)));
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };
  const chooseFolder = async (which: "client" | "server") => {
    const picked = await open({ directory: true, multiple: false, defaultPath: (which === "client" ? clientRoot : serverRoot) || undefined, title: which === "client" ? "The client's element\\maps folder" : "The server's map config folder (gamed\\config, holding a folder per map)" });
    if (typeof picked !== "string") return;
    if (which === "client") setClientRoot(picked);
    else { setServerRoot(picked); onServerRoot(picked); }
  };
  const sync = async () => {
    if (!rows) return;
    const chosen = rows.filter((row) => row.canSync && picked.has(row.clientPath!));
    if (!chosen.length || !window.confirm(`Write ${chosen.length} server file${chosen.length === 1 ? "" : "s"} from the client files?\n\nEach gets the client file's timestamp and values (buff regions are kept); the client files are not changed, so clients need no update.${backup ? " The old server files are backed up." : ""}`)) return;
    setBusy(true);
    setError(null);
    const failed: string[] = [];
    for (const row of chosen) {
      try { await syncMapServer(row.clientPath!, row.serverPath!, backup); } catch (problem) { failed.push(`${row.folder} ${row.kind}: ${String(problem).replace(/^Error: /, "")}`); }
    }
    setBusy(false);
    setNote(`Wrote ${chosen.length - failed.length} server file${chosen.length - failed.length === 1 ? "" : "s"}. Restart the game servers of those maps.${failed.length ? ` Failed: ${failed.join("; ")}` : ""}`);
    await scan();
  };

  const shown = (rows ?? []).filter((row) => filter === "all" || (filter === "missing" ? status(row) === "no server file" : status(row) !== "in step" && status(row) !== "no server file"));
  const counts = useMemo(() => {
    const out = new Map<string, number>();
    for (const row of rows ?? []) out.set(status(row), (out.get(status(row)) ?? 0) + 1);
    return out;
  }, [rows]);
  const syncable = shown.filter((row) => row.canSync);

  return <div className="modal-backdrop" onMouseDown={() => !busy && onClose()}>
    <div className="modal ai-sim-dialog maps-scan-dialog" role="dialog" aria-label="Check all maps" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Check all maps</h3><span className="spacer" /><button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={16} /></button></header>
      <div className="maps-scan-folders">
        <span className="npcgen-label">Client maps</span><span className="mono truncate" title={clientRoot}>{clientRoot || <span className="muted">not chosen</span>}</span><button className="btn small" onClick={() => void chooseFolder("client")}><FolderOpen size={12} /> Choose…</button>
        <span className="npcgen-label">Server config</span><span className="mono truncate" title={serverRoot}>{serverRoot || <span className="muted">not chosen (client files only)</span>}</span><button className="btn small" onClick={() => void chooseFolder("server")}><FolderOpen size={12} /> Choose…</button>
      </div>
      <div className="npcgen-nearby-bar">
        <button className="btn small primary" onClick={() => void scan()} disabled={busy || !clientRoot}>{busy ? <Loader2 size={12} className="spin" /> : <RefreshCw size={12} />} {rows ? "Scan again" : "Scan"}</button>
        {rows && <span className="dyn-compare-tabs">
          <button className={"btn small" + (filter === "attention" ? " active" : "")} onClick={() => setFilter("attention")}>Needs attention <span className="muted">{count(rows.length - (counts.get("in step") ?? 0) - (counts.get("no server file") ?? 0))}</span></button>
          <button className={"btn small" + (filter === "missing" ? " active" : "")} onClick={() => setFilter("missing")} title="Client maps without a server file: maps the server does not host, or a server folder named differently">No server file <span className="muted">{count(counts.get("no server file") ?? 0)}</span></button>
          <button className={"btn small" + (filter === "all" ? " active" : "")} onClick={() => setFilter("all")}>All <span className="muted">{count(rows.length)}</span></button>
        </span>}
        {rows && <span className="muted small">{[...counts.entries()].map(([key, value]) => `${value} ${key}`).join(" · ")}</span>}
      </div>
      {error && <div className="path-data-message error"><CircleAlert size={14} /> {error}</div>}
      {note && <div className="path-data-message"><Check size={14} /> {note}</div>}
      <div className="maps-scan-list">
        {!rows ? <div className="empty-note">{busy ? "Scanning…" : "Choose the folders and scan. Server folders are found by map name (or the instance's data path)."}</div> : <table className="dyn-table maps-scan-table">
          <thead><tr>
            <th><input type="checkbox" title="Pick every server file shown that can be written" checked={syncable.length > 0 && syncable.every((row) => picked.has(row.clientPath!))} onChange={(event) => setPicked((current) => { const next = new Set(current); for (const row of syncable) { if (event.target.checked) next.add(row.clientPath!); else next.delete(row.clientPath!); } return next; })} /></th>
            <th>Map</th><th>File</th><th>Status</th><th>Details</th><th />
          </tr></thead>
          <tbody>{shown.map((row) => {
            const state = status(row);
            return <tr key={`${row.folder}:${row.kind}`}>
              <td>{row.canSync ? <input type="checkbox" checked={picked.has(row.clientPath!)} onChange={() => setPicked((current) => { const next = new Set(current); if (next.has(row.clientPath!)) next.delete(row.clientPath!); else next.add(row.clientPath!); return next; })} /> : null}</td>
              <td><span className="mono">{row.folder}</span> <span className="muted small">{names.get(row.folder.toLowerCase()) ?? ""}</span></td>
              <td className="mono small">{row.kind}.clt{row.clientVersion !== null ? ` v${row.clientVersion}` : ""}</td>
              <td><span className={"tag" + (state === "in step" ? " ok" : state === "unreadable" || state === "differs" ? " danger" : " warn")}>{state}</span></td>
              <td className="small">
                {row.clientError && <div className="ai-warn">{row.clientError.replace(/^.*?\.clt: /, "")}</div>}
                {row.serverError && row.serverError !== "missing" && <div className="ai-warn">{row.serverError}</div>}
                {row.differences.length > 0 && <div>{row.differences.join("; ")}</div>}
                {(row.errors > 0 || row.warnings > 0) && <div className="muted">{row.errors ? <><CircleAlert size={11} className="danger-icon" /> {row.errors} error{row.errors === 1 ? "" : "s"} </> : null}{row.warnings ? <><TriangleAlert size={11} /> {row.warnings} warning{row.warnings === 1 ? "" : "s"}</> : null}</div>}
              </td>
              <td>{row.clientPath && !row.clientError && <button className="btn small" onClick={() => onOpen(row.clientPath!)}>Open</button>}</td>
            </tr>;
          })}</tbody>
        </table>}
        {rows && !shown.length && <div className="empty-note">{filter === "attention" ? "Every map the server hosts is in step." : "None."}</div>}
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">Writing a server file uses the client file's timestamp and values and keeps the server's buff regions; the client files are not changed. Maps the server does not host (no folder) are left alone.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose} disabled={busy}>Close</button>
        <button className="btn primary" disabled={busy || !rows || !shown.some((row) => row.canSync && picked.has(row.clientPath!))} onClick={() => void sync()}><Check size={14} /> Write {count(shown.filter((row) => row.canSync && picked.has(row.clientPath!)).length)} server files</button>
      </footer>
    </div>
  </div>;
}
