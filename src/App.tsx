import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getRecord, getSettings, getTalk, iconUrl, listRecords, listTalks, openElements } from "./elements/api";
import type { FileSummary, FindHit, ListSummary, RecordDetail, RecordRow, SettingsView, TalkDetail } from "./elements/types";
import { bytes, count } from "./elements/format";
import { ListSidebar } from "./components/ListSidebar";
import { RecordTable } from "./components/RecordTable";
import { type FieldFocus, RecordInspector } from "./components/RecordInspector";
import { AdvancedSearch } from "./components/AdvancedSearch";
import { ProblemsPanel } from "./components/ProblemsPanel";
import { SchemaEditor } from "./components/SchemaEditor";
import { TabBar, type TabLabel } from "./components/TabBar";
import { SettingsDialog } from "./components/SettingsDialog";
import { SetsEditor } from "./components/SetsEditor";
import { FindPalette } from "./components/FindPalette";
import { DialogViewer } from "./components/DialogViewer";
import type { FieldSpec } from "./schema/model";
import { DIALOGS, EMPTY_TABS, type Location, type Tab, loadTabs, makeTab, saveTabs, tabsReducer } from "./tabs";
import "./App.css";
import logo from "./assets/logo.png";
import { Braces, ChevronLeft, ChevronRight, CircleAlert, FolderOpen, Gem, ListFilter, Search, Settings } from "lucide-react";

/** The last Find: its hits are stepped through with F3 / Shift+F3. */
interface LastFind {
  query: string;
  hits: FindHit[];
  position: number;
}

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

/** Rows of a list, or of the NPC dialogs (named by their first words). */
async function loadRows(list: number): Promise<RecordRow[]> {
  if (list !== DIALOGS) return listRecords(list);
  const talks = await listTalks();
  return talks.map((t) => ({ index: t.index, id: t.id, name: t.title }));
}

/** The NPC dialogs, shown in the record table like a list. */
function dialogsList(summary: FileSummary): ListSummary {
  return {
    index: DIALOGS,
    name: "NPC Dialogs",
    key: null,
    structName: "TALK_PROC",
    itemSize: 0,
    count: summary.talkCount,
    offset: 0,
    layout: "exact",
    layoutId: null,
    layoutSize: null,
    custom: false,
  };
}

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
  const [talk, setTalk] = useState<TalkDetail | null>(null);
  const [lastPath, setLastPath] = useState<string | null>(readLastPath);
  const rowsCache = useRef(new Map<number, RecordRow[]>());
  // Bumped when rows load in the background, so tab titles can use names.
  const [, setRowsLoaded] = useState(0);
  const [tabs, dispatch] = useReducer(tabsReducer, EMPTY_TABS);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorIntent, setEditorIntent] = useState<{ list: number; offset: number; spec: FieldSpec } | null>(null);
  const [settingsView, setSettingsView] = useState<SettingsView | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // The enums & masks editor, open at a set (or none).
  const [setsEditor, setSetsEditor] = useState<{ key: string | null } | null>(null);
  const started = useRef(false);
  const iconGen = settingsView?.client?.hasItemIcons ? settingsView.iconGeneration : null;
  const [findOpen, setFindOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  // The left side shows the lists, the search or the problems.
  const [problemsOpen, setProblemsOpen] = useState(false);
  // Mounted once opened, so a scan is not repeated on every toggle.
  const [problemsMounted, setProblemsMounted] = useState(false);
  const [problemCounts, setProblemCounts] = useState<{ errors: number; warnings: number } | null>(null);
  useEffect(() => {
    setProblemsMounted(false);
    setProblemsOpen(false);
    setProblemCounts(null);
  }, [summary?.path]);
  const toggleSearch = () => {
    setProblemsOpen(false);
    setSearchOpen((o) => !o);
  };
  const toggleProblems = () => {
    setSearchOpen(false);
    setProblemsMounted(true);
    setProblemsOpen((o) => !o);
  };
  const [focus, setFocus] = useState<FieldFocus | null>(null);
  const [lastFind, setLastFind] = useState<LastFind | null>(null);
  // Hits belong to one file.
  useEffect(() => setLastFind(null), [summary?.path]);

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
      const restored = loadTabs(path, result.lists.map((l) => l.count), result.talkCount);
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
    if (listIndex === null || (listIndex < 0 && listIndex !== DIALOGS)) {
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
    loadRows(listIndex)
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
      setTalk(null);
      return;
    }
    let cancelled = false;
    if (listIndex === DIALOGS) {
      setDetail(null);
      getTalk(recordIndex)
        .then((result) => !cancelled && setTalk(result))
        .catch((e) => !cancelled && setError(String(e)));
    } else {
      setTalk(null);
      getRecord(listIndex, recordIndex)
        .then((result) => !cancelled && setDetail(result))
        .catch((e) => !cancelled && setError(String(e)));
    }
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
        const result = await loadRows(list).catch(() => null);
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

  /** Opens the next (or previous) hit of the last Find. */
  const stepFind = (delta: number) => {
    if (!lastFind?.hits.length) return setFindOpen(true);
    const n = lastFind.hits.length;
    const position = (lastFind.position + delta + n) % n;
    const hit = lastFind.hits[position];
    setLastFind({ ...lastFind, position });
    follow(hit.list, hit.index);
  };
  const stepFindRef = useRef(stepFind);
  stepFindRef.current = stepFind;
  const toggleSearchRef = useRef(toggleSearch);
  toggleSearchRef.current = toggleSearch;
  const toggleProblemsRef = useRef(toggleProblems);
  toggleProblemsRef.current = toggleProblems;

  // Tab, history and find shortcuts (not while the schema editor is open).
  useEffect(() => {
    if (!summary || editorOpen) return;
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.shiftKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        toggleSearchRef.current();
      } else if (mod && e.shiftKey && e.key.toLowerCase() === "m") {
        e.preventDefault();
        toggleProblemsRef.current();
      } else if (mod && e.key.toLowerCase() === "g") {
        e.preventDefault();
        setFindOpen(true);
      } else if (e.key === "F3") {
        e.preventDefault();
        stepFindRef.current(e.shiftKey ? -1 : 1);
      } else if (e.altKey && e.key === "ArrowLeft") {
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
    if (tab.list === DIALOGS) {
      if (tab.row === null) return { title: "NPC Dialogs", subtitle: "dialogs" };
      const row = rowsCache.current.get(DIALOGS)?.[tab.row];
      return { title: row?.name || `Dialog #${tab.row}`, subtitle: `NPC dialog · ID ${row?.id ?? "?"}` };
    }
    const l = summary?.lists[tab.list];
    const listName = l?.name ?? `List ${tab.list}`;
    if (tab.row === null) return { title: listName, subtitle: `list ${tab.list}` };
    const row = rowsCache.current.get(tab.list)?.[tab.row];
    return { title: row?.name || `${listName} #${tab.row}`, subtitle: `${listName} · #${tab.row}`, icon: icon(row?.icon) };
  };

  const showingDialogs = listIndex === DIALOGS;
  const list = summary && listIndex !== null && listIndex >= 0 ? summary.lists[listIndex] : summary && showingDialogs ? dialogsList(summary) : null;
  const row = rows && recordIndex !== null ? (rows[recordIndex] ?? null) : null;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img className="logo" src={logo} alt="" draggable={false} />
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
        {summary && (
          <div className="find-bar">
            <button className="find-trigger" onClick={() => setFindOpen(true)} title="Find a record by ID or name in every list (Ctrl+G)">
              <Search size={14} />
              <span className="truncate">{lastFind ? lastFind.query : "Find by ID or name…"}</span>
              <kbd>Ctrl G</kbd>
            </button>
            <button
              className={"icon-btn small" + (searchOpen ? " active" : "")}
              onClick={toggleSearch}
              title="Advanced search: conditions on fields, or a value in any field (Ctrl+Shift+F)"
              aria-label="Advanced search"
            >
              <ListFilter size={15} />
            </button>
            <button
              className={"icon-btn small problems-btn" + (problemsOpen ? " active" : "")}
              onClick={toggleProblems}
              title={
                problemCounts
                  ? `Problems: ${problemCounts.errors} error(s), ${problemCounts.warnings} warning(s) (Ctrl+Shift+M)`
                  : "Problems: scan the file for broken references, duplicate IDs and more (Ctrl+Shift+M)"
              }
              aria-label="Problems"
            >
              <CircleAlert size={15} />
              {problemCounts && problemCounts.errors + problemCounts.warnings > 0 && (
                <span className={"problems-badge" + (problemCounts.errors ? " error" : " warning")}>
                  {problemCounts.errors || problemCounts.warnings}
                </span>
              )}
            </button>
            {lastFind && lastFind.hits.length > 1 && (
              <>
                <span className="find-pos mono muted">
                  {lastFind.position + 1}/{lastFind.hits.length}
                </span>
                <button className="icon-btn small" onClick={() => stepFind(-1)} title="Previous result (Shift+F3)">
                  <ChevronLeft size={15} />
                </button>
                <button className="icon-btn small" onClick={() => stepFind(1)} title="Next result (F3)">
                  <ChevronRight size={15} />
                </button>
              </>
            )}
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

      {findOpen && summary && (
        <FindPalette
          lists={summary.lists}
          initialQuery={lastFind?.query ?? ""}
          icon={icon}
          onOpen={(hit, newTab) => follow(hit.list, hit.index, newTab)}
          onResults={(query, hits, position) => setLastFind({ query, hits, position })}
          onClose={() => setFindOpen(false)}
        />
      )}

      {setsEditor && (
        <SetsEditor
          initialKey={setsEditor.key}
          onChanged={(next) => next && onSchemaSaved(next)}
          onClose={() => setSetsEditor(null)}
        />
      )}

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
            initialList={editorIntent?.list ?? (list && list.index >= 0 ? list.index : 0)}
            intent={editorIntent}
            initialRow={showingDialogs ? 0 : (recordIndex ?? 0)}
            onEditSets={(key) => setSetsEditor({ key })}
            onSaved={onSchemaSaved}
            onClose={closeEditor}
          />
        </main>
      ) : summary ? (
        <main className={"workspace" + (searchOpen || problemsOpen ? " searching" : "")}>
          {/* Kept mounted while closed, so the search and its results stay. */}
          <div className="search-slot" hidden={!searchOpen}>
            <AdvancedSearch
              key={summary.path}
              lists={summary.lists}
              currentList={listIndex}
              icon={icon}
              onOpen={(hit, off, newTab) => {
                openLocation({ list: hit.list, row: hit.row }, newTab ? { newTab: true } : {});
                if (off !== null) setFocus({ list: hit.list, row: hit.row, off, nonce: Date.now() });
              }}
              onClose={() => setSearchOpen(false)}
            />
          </div>
          {problemsMounted && (
            <div className="search-slot" hidden={!problemsOpen}>
              <ProblemsPanel
                key={summary.path}
                lists={summary.lists}
                icon={icon}
                onCounts={(errors, warnings) => setProblemCounts({ errors, warnings })}
                onOpen={(p, newTab) => {
                  const options = newTab ? { newTab: true } : {};
                  if (p.talk !== undefined) openLocation({ list: DIALOGS, row: p.talk }, options);
                  else if (p.list !== undefined && p.row === undefined) openLocation({ list: p.list, row: null }, options);
                  else if (p.list !== undefined && p.row !== undefined) {
                    openLocation({ list: p.list, row: p.row }, options);
                    if (p.off !== undefined) setFocus({ list: p.list, row: p.row, off: p.off, nonce: Date.now() });
                  }
                }}
                onClose={() => setProblemsOpen(false)}
              />
            </div>
          )}
          {!searchOpen && !problemsOpen && (
            <ListSidebar lists={summary.lists} selected={listIndex} onSelect={selectList} talkCount={summary.talkCount} />
          )}
          {searchOpen || problemsOpen ? null : list ? (
            <RecordTable
              list={list}
              rows={rows}
              selected={recordIndex}
              onSelect={(index) => openLocation({ list: list.index, row: index })}
              onOpen={(index) => openLocation({ list: list.index, row: index }, { pin: true })}
              icon={icon}
              meta={showingDialogs ? `${count(summary.talkCount)} dialogs` : undefined}
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
            {showingDialogs ? (
              <DialogViewer
                detail={talk}
                lists={summary.lists}
                canGoBack={(activeTab?.history.length ?? 0) > 0}
                onBack={() => dispatch({ type: "back" })}
                onFollow={follow}
              />
            ) : list ? (
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
                onEditSet={(key) => setSetsEditor({ key })}
                lists={summary.lists}
                focus={focus}
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
            <img className="drop-logo" src={logo} alt="" draggable={false} />
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
