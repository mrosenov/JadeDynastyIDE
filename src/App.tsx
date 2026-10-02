import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getRecord, getSettings, iconUrl, listRecords, openElements } from "./elements/api";
import type { FileSummary, RecordDetail, RecordRow, SettingsView } from "./elements/types";
import { bytes, count } from "./elements/format";
import { ListSidebar } from "./components/ListSidebar";
import { RecordTable } from "./components/RecordTable";
import { RecordInspector } from "./components/RecordInspector";
import { SchemaEditor } from "./components/SchemaEditor";
import { TabBar, type TabLabel } from "./components/TabBar";
import { SettingsDialog } from "./components/SettingsDialog";
import type { FieldSpec } from "./schema/model";
import { EMPTY_TABS, type Location, type Tab, loadTabs, makeTab, saveTabs, tabsReducer } from "./tabs";
import "./App.css";
import { Braces, FolderOpen, Gem, Settings } from "lucide-react";

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

function parseLabel(summary: FileSummary): { text: string; tone: "ok" | "warn" | "muted"; title: string } {
  switch (summary.parseMode) {
    case "layout":
      return {
        text: `Layout ${summary.layoutId}`,
        tone: summary.layoutUnverified ? "warn" : "ok",
        title:
          (summary.layoutSource ?? "") +
          (summary.layoutUnverified ? "\nThe list count of this layout was never checked against a real file." : ""),
      };
    case "markers":
      return {
        text: `Markers from ${summary.markersFrom}`,
        tone: "warn",
        title: `No layout exists for v${summary.version}. Lists were split using the ${summary.markersFrom} marker table, and names and fields are borrowed by record size.`,
      };
    case "detected":
      return {
        text: "Detected",
        tone: "muted",
        title: "No known marker table fits this file. Lists were found by recognising each segment, and names and fields are borrowed by record size.",
      };
  }
}

export default function App() {
  const [summary, setSummary] = useState<FileSummary | null>(null);
  const [loading, setLoading] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rows, setRows] = useState<RecordRow[] | null>(null);
  const [detail, setDetail] = useState<RecordDetail | null>(null);
  const [lastPath, setLastPath] = useState<string | null>(readLastPath);
  const rowsCache = useRef(new Map<number, RecordRow[]>());
  // Bumped when rows load in the background, so tab titles can use names.
  const [, setRowsLoaded] = useState(0);
  const [tabs, dispatch] = useReducer(tabsReducer, EMPTY_TABS);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorIntent, setEditorIntent] = useState<{ list: number; offset: number; spec: FieldSpec } | null>(null);
  const [settingsView, setSettingsView] = useState<SettingsView | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const started = useRef(false);
  const iconGen = settingsView?.client?.hasItemIcons ? settingsView.iconGeneration : null;
  const icon = (pathId?: number | null) => (iconGen !== null && pathId ? iconUrl(iconGen, pathId) : undefined);

  // The active tab decides what the sidebar, the table and the inspector show.
  const activeTab = tabs.tabs.find((t) => t.id === tabs.active) ?? null;
  const listIndex = activeTab?.list ?? null;
  const recordIndex = activeTab?.row ?? null;

  const loadFile = useCallback(async (path: string) => {
    setLoading(path);
    setError(null);
    try {
      const result = await openElements(path);
      rowsCache.current.clear();
      setSummary(result);
      setRows(null);
      setDetail(null);
      const first = result.lists.findIndex((l) => l.count > 0);
      const restored = loadTabs(path, result.lists.map((l) => l.count));
      dispatch({
        type: "reset",
        state:
          restored ??
          (first < 0 ? EMPTY_TABS : (() => {
            const tab = makeTab({ list: first, row: null }, false);
            return { tabs: [tab], active: tab.id };
          })()),
      });
      writeLastPath(path);
      setLastPath(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(null);
    }
  }, []);

  // Settings, and the client's elements.data on start when asked for.
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    getSettings()
      .then((view) => {
        setSettingsView(view);
        const elements = view.client?.elementsPath;
        if (view.settings.openOnStart && elements) loadFile(elements);
      })
      .catch((e) => setError(String(e)));
  }, [loadFile]);

  // New client settings: reload rows and the record so icons and paths appear.
  const onSettingsSaved = (view: SettingsView) => {
    setSettingsView(view);
    rowsCache.current.clear();
    setSummary((s) => (s ? { ...s } : s));
  };

  // Remember the open tabs per file.
  useEffect(() => {
    if (summary) saveTabs(summary.path, tabs);
  }, [summary, tabs]);

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

  // Load the records of the active tab's list.
  useEffect(() => {
    if (listIndex === null || listIndex < 0) {
      setRows(null);
      return;
    }
    const show = (result: RecordRow[]) => {
      setRows(result);
      dispatch({ type: "settle", row: result.length ? 0 : null });
    };
    const cached = rowsCache.current.get(listIndex);
    if (cached) {
      show(cached);
      return;
    }
    setRows(null);
    let cancelled = false;
    listRecords(listIndex)
      .then((result) => {
        rowsCache.current.set(listIndex, result);
        if (!cancelled) show(result);
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [listIndex, summary]);

  // Load the active record.
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
  }, [listIndex, recordIndex, summary]);

  // Load record names for the other tabs' lists, for their titles.
  useEffect(() => {
    if (!summary) return;
    const missing = [...new Set(tabs.tabs.map((t) => t.list))].filter((l) => !rowsCache.current.has(l));
    let cancelled = false;
    (async () => {
      for (const list of missing) {
        const result = await listRecords(list).catch(() => null);
        if (cancelled || !result) return;
        rowsCache.current.set(list, result);
        setRowsLoaded((n) => n + 1);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [tabs.tabs, summary]);

  const openLocation = (loc: Location, options: { pin?: boolean; newTab?: boolean } = {}) =>
    dispatch({ type: "open", loc, ...options });

  const selectList = (index: number) => openLocation({ list: index, row: null });

  const follow = (list: number, row: number, newTab = false) =>
    newTab ? openLocation({ list, row }, { newTab: true }) : dispatch({ type: "navigate", loc: { list, row } });

  // A schema was saved: the file was re-read with the new layouts. The tabs
  // keep their records while everything reloads.
  const onSchemaSaved = (next: FileSummary) => {
    rowsCache.current.clear();
    setSummary(next);
  };

  const closeEditor = (list: number) => {
    setEditorOpen(false);
    setEditorIntent(null);
    if (list !== listIndex) selectList(list);
  };

  // Tab and history shortcuts (not while the schema editor is open).
  useEffect(() => {
    if (!summary || editorOpen) return;
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        dispatch({ type: "back" });
      } else if (mod && e.key === "Tab") {
        e.preventDefault();
        dispatch({ type: "cycle", delta: e.shiftKey ? -1 : 1 });
      } else if (mod && e.key.toLowerCase() === "w") {
        e.preventDefault();
        if (tabs.active !== null) dispatch({ type: "close", id: tabs.active });
      } else if (mod && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        dispatch({ type: "activateIndex", index: e.key === "9" ? -1 : Number(e.key) - 1 });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [summary, editorOpen, tabs.active]);

  const tabLabel = (tab: Tab): TabLabel => {
    const l = summary?.lists[tab.list];
    const listName = l?.name ?? `List ${tab.list}`;
    if (tab.row === null) return { title: listName, subtitle: `list ${tab.list}` };
    const row = rowsCache.current.get(tab.list)?.[tab.row];
    return { title: row?.name || `${listName} #${tab.row}`, subtitle: `${listName} · #${tab.row}`, icon: icon(row?.icon) };
  };

  const list = summary && listIndex !== null && listIndex >= 0 ? summary.lists[listIndex] : null;
  const row = rows && recordIndex !== null ? (rows[recordIndex] ?? null) : null;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="logo" aria-hidden>
            <Gem size={18} />
          </span>
          JD IDE
        </div>
        <button className="btn primary" onClick={chooseFile} title="Open elements.data (Ctrl+O)">
          <FolderOpen size={15} /> Open…
        </button>
        {summary && (
          <div className="file-chip" title={summary.path}>
            <span className="truncate">{fileName(summary.path)}</span>
            <span className="tag">v{summary.version}</span>
            {(() => {
              const p = parseLabel(summary);
              return (
                <span className={`tag ${p.tone}`} title={p.title}>
                  {p.text}
                </span>
              );
            })()}
          </div>
        )}
        <span className="spacer" />
        {loading && <span className="muted">Reading {fileName(loading)}…</span>}
        <button
          className={"btn" + (editorOpen ? " active" : "")}
          onClick={() => (editorOpen ? closeEditor(listIndex ?? 0) : setEditorOpen(true))}
          disabled={!summary || !summary.lists.length}
          title={summary ? "Write your own schema for this file's lists" : "Open a file to edit its schema"}
        >
          <Braces size={15} /> Schema editor
        </button>
        <button
          className={"icon-btn topbar-settings" + (settingsOpen ? " active" : "")}
          onClick={() => setSettingsOpen(true)}
          title={
            settingsView?.client
              ? `Settings · client: ${settingsView.client.root}`
              : "Settings: set the game client folder for icons and quick access to its files"
          }
          aria-label="Settings"
        >
          <Settings size={18} />
        </button>
      </header>

      {settingsOpen && settingsView && (
        <SettingsDialog
          view={settingsView}
          onSaved={onSettingsSaved}
          onOpenFile={loadFile}
          onClose={() => setSettingsOpen(false)}
        />
      )}

      {error && (
        <div className="error-bar" role="alert">
          <span>{error}</span>
          <button className="link" onClick={() => setError(null)}>
            Dismiss
          </button>
        </div>
      )}

      {summary && editorOpen ? (
        <main className="editor-main">
          <SchemaEditor
            summary={summary}
            initialList={editorIntent?.list ?? list?.index ?? 0}
            intent={editorIntent}
            initialRow={recordIndex ?? 0}
            onSaved={onSchemaSaved}
            onClose={closeEditor}
          />
        </main>
      ) : summary ? (
        <main className="workspace">
          <ListSidebar lists={summary.lists} selected={listIndex} onSelect={selectList} />
          {list ? (
            <RecordTable
              list={list}
              rows={rows}
              selected={recordIndex}
              onSelect={(index) => openLocation({ list: list.index, row: index })}
              onOpen={(index) => openLocation({ list: list.index, row: index }, { pin: true })}
              icon={icon}
            />
          ) : (
            <section className="pane records">
              <div className="empty-note center">Pick a list to see its records.</div>
            </section>
          )}
          <div className="inspector-column">
            <TabBar
              tabs={tabs.tabs}
              active={tabs.active}
              label={tabLabel}
              onActivate={(id) => dispatch({ type: "activate", id })}
              onPin={(id) => dispatch({ type: "pin", id })}
              onClose={(id) => dispatch({ type: "close", id })}
            />
            {list ? (
              <RecordInspector
                list={list}
                row={row}
                detail={detail}
                canGoBack={(activeTab?.history.length ?? 0) > 0}
                onBack={() => dispatch({ type: "back" })}
                onFollow={follow}
                onDefine={(list, offset, spec) => {
                  setEditorIntent({ list, offset, spec });
                  setEditorOpen(true);
                }}
                icon={icon}
              />
            ) : (
              <section className="pane inspector">
                <div className="empty-note center">No record open. Pick a list, then a record.</div>
              </section>
            )}
          </div>
        </main>
      ) : (
        <main className="welcome">
          <div className="drop-card">
            <div className="drop-icon" aria-hidden>
              <Gem size={30} />
            </div>
            <h1>Open an elements.data file</h1>
            <p className="muted">
              Drop a file anywhere in this window, or choose one. Layouts are built in for versions 66, 112, 156,
              158, 160, 165 and 176. Other versions still open: lists are matched to known ones by record size.
            </p>
            <div className="drop-actions">
              {settingsView?.client?.elementsPath && (
                <button
                  className="btn primary"
                  onClick={() => loadFile(settingsView.client!.elementsPath!)}
                  title={settingsView.client.elementsPath}
                >
                  <Gem size={15} /> Open client elements.data
                </button>
              )}
              <button className={"btn" + (settingsView?.client?.elementsPath ? "" : " primary")} onClick={chooseFile}>
                <FolderOpen size={15} /> Choose file…
              </button>
              {lastPath && (
                <button className="btn" onClick={() => loadFile(lastPath)} title={lastPath}>
                  Reopen {fileName(lastPath)}
                </button>
              )}
            </div>
            {lastPath && <div className="muted small truncate">{lastPath}</div>}
            {settingsView && !settingsView.client && (
              <button className="link" onClick={() => setSettingsOpen(true)}>
                <Settings size={13} /> Set your game client folder to open its files quickly and show item icons
              </button>
            )}
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
            {tabs.tabs.length > 0 && (
              <span title="Ctrl+Tab switches tabs, Ctrl+W closes, Ctrl+1…9 jumps">
                {tabs.tabs.length} tab{tabs.tabs.length > 1 ? "s" : ""}
              </span>
            )}
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
