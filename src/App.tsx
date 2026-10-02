import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getRecord, listRecords, openElements } from "./elements/api";
import type { FileSummary, RecordDetail, RecordRow } from "./elements/types";
import { bytes, count } from "./elements/format";
import { ListSidebar } from "./components/ListSidebar";
import { RecordTable } from "./components/RecordTable";
import { RecordInspector } from "./components/RecordInspector";
import "./App.css";

const LAST_PATH_KEY = "jdide.lastPath";

function readLastPath(): string | null {
  try {
    return localStorage.getItem(LAST_PATH_KEY);
  } catch {
    return null;
  }
}

function writeLastPath(path: string) {
  try {
    localStorage.setItem(LAST_PATH_KEY, path);
  } catch {
    // Remembering the last file is a convenience only.
  }
}

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

export default function App() {
  const [summary, setSummary] = useState<FileSummary | null>(null);
  const [loading, setLoading] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [listIndex, setListIndex] = useState<number | null>(null);
  const [rows, setRows] = useState<RecordRow[] | null>(null);
  const [recordIndex, setRecordIndex] = useState<number | null>(null);
  const [detail, setDetail] = useState<RecordDetail | null>(null);
  const [lastPath, setLastPath] = useState<string | null>(readLastPath);
  const rowsCache = useRef(new Map<number, RecordRow[]>());

  const loadFile = useCallback(async (path: string) => {
    setLoading(path);
    setError(null);
    try {
      const result = await openElements(path);
      rowsCache.current.clear();
      setSummary(result);
      setRows(null);
      setDetail(null);
      setRecordIndex(null);
      setListIndex(result.lists.findIndex((l) => l.count > 0));
      writeLastPath(path);
      setLastPath(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(null);
    }
  }, []);

  const chooseFile = useCallback(async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "Element data", extensions: ["data"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (typeof picked === "string") await loadFile(picked);
  }, [loadFile]);

  // Ctrl+O and drag-and-drop open a file.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "o") {
        e.preventDefault();
        chooseFile();
      }
    };
    window.addEventListener("keydown", onKey);
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop" && event.payload.paths.length) loadFile(event.payload.paths[0]);
    });
    return () => {
      window.removeEventListener("keydown", onKey);
      unlisten.then((f) => f());
    };
  }, [chooseFile, loadFile]);

  // Load the records of the selected list.
  useEffect(() => {
    if (listIndex === null || listIndex < 0) return;
    setDetail(null);
    const cached = rowsCache.current.get(listIndex);
    if (cached) {
      setRows(cached);
      setRecordIndex(cached.length ? 0 : null);
      return;
    }
    setRows(null);
    let cancelled = false;
    listRecords(listIndex)
      .then((result) => {
        rowsCache.current.set(listIndex, result);
        if (cancelled) return;
        setRows(result);
        setRecordIndex(result.length ? 0 : null);
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [listIndex, summary]);

  // Load the selected record.
  useEffect(() => {
    if (listIndex === null || recordIndex === null) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    getRecord(listIndex, recordIndex)
      .then((result) => !cancelled && setDetail(result))
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [listIndex, recordIndex]);

  const selectList = (index: number) => {
    setRecordIndex(null);
    setListIndex(index);
  };

  const list = summary && listIndex !== null && listIndex >= 0 ? summary.lists[listIndex] : null;
  const row = rows && recordIndex !== null ? (rows[recordIndex] ?? null) : null;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="logo" aria-hidden>
            ◆
          </span>
          JD IDE
        </div>
        <button className="btn primary" onClick={chooseFile} title="Open elements.data (Ctrl+O)">
          Open…
        </button>
        {summary && (
          <div className="file-chip" title={summary.path}>
            <span className="truncate">{fileName(summary.path)}</span>
            <span className="tag">v{summary.version}</span>
            {summary.profileVersion !== null && (
              <span
                className={"tag " + (summary.profileExact ? "ok" : "warn")}
                title={summary.profileSource ?? undefined}
              >
                {summary.profileExact ? "Profile v" : "Borrowing v"}
                {summary.profileVersion}
              </span>
            )}
            {summary.profileVersion === null && (
              <span className="tag muted" title="No list names or layouts are known for this version">
                Raw view
              </span>
            )}
          </div>
        )}
        <span className="spacer" />
        {loading && <span className="muted">Reading {fileName(loading)}…</span>}
      </header>

      {error && (
        <div className="error-bar" role="alert">
          <span>{error}</span>
          <button className="link" onClick={() => setError(null)}>
            Dismiss
          </button>
        </div>
      )}

      {summary && list ? (
        <main className="workspace">
          <ListSidebar lists={summary.lists} selected={listIndex} onSelect={selectList} />
          <RecordTable list={list} rows={rows} selected={recordIndex} onSelect={setRecordIndex} />
          <RecordInspector list={list} row={row} detail={detail} />
        </main>
      ) : (
        <main className="welcome">
          <div className="drop-card">
            <div className="drop-icon" aria-hidden>
              ◆
            </div>
            <h1>Open an elements.data file</h1>
            <p className="muted">
              Drop a file anywhere in this window, or choose one. Versions 112 and 156 have full list names and
              layouts. Other versions open in a raw view, borrowing names where the layout matches.
            </p>
            <div className="drop-actions">
              <button className="btn primary" onClick={chooseFile}>
                Choose file…
              </button>
              {lastPath && (
                <button className="btn" onClick={() => loadFile(lastPath)} title={lastPath}>
                  Reopen {fileName(lastPath)}
                </button>
              )}
            </div>
            {lastPath && <div className="muted small truncate">{lastPath}</div>}
          </div>
        </main>
      )}

      <footer className="statusbar">
        {summary ? (
          <>
            <span>{bytes(summary.fileSize)}</span>
            <span>{summary.lists.length} lists</span>
            <span>{count(summary.lists.reduce((n, l) => n + l.count, 0))} records</span>
            <span>{count(summary.talkCount)} NPC dialogs</span>
            <span title="Export time stored in the file header">
              Exported {new Date(summary.timestamp * 1000).toLocaleString()}
            </span>
            {summary.exporter && <span title="Exporter machine name stored in the file">by {summary.exporter}</span>}
            <span className="spacer" />
            <span className="mono" title="Raw version word">
              0x{summary.rawVersion.toString(16).toUpperCase()}
            </span>
          </>
        ) : (
          <span>Ready</span>
        )}
      </footer>
    </div>
  );
}
