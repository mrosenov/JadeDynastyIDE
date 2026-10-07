import { forwardRef, useCallback, useDeferredValue, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FileCheck2, FolderOpen, ListTree, Minus, Plus, Search, ShieldCheck } from "lucide-react";
import { getTask, openTasks } from "../elements/api";
import { bytes, count } from "../elements/format";
import type { TaskDetail, TaskFieldView, TaskRootSummary, TasksFileSummary, TaskTreeNode } from "../elements/types";

export interface TasksEditorState {
  loaded: boolean;
  path: string | null;
  summary: TasksFileSummary | null;
}

export interface TasksEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
}

interface Props {
  active: boolean;
  defaultPath?: string | null;
  onStateChange: (state: TasksEditorState) => void;
}

const PAGE_SIZE = 200;
const pathKey = (path: number[]) => path.join(".");
const rootKey = (root: TaskRootSummary) => `${root.pack}:${root.root}`;
const branchKey = (root: TaskRootSummary, path: number[]) => `${rootKey(root)}:${pathKey(path)}`;
const label = (name: string) => name.replace(/^unknown_/, "unknown · ").replaceAll("_", " ");

function NestedTaskRow({ root, node, selected, expanded, onSelect, onToggle, depth = 1 }: { root: TaskRootSummary; node: TaskTreeNode; selected: string | null; expanded: Set<string>; onSelect: (path: number[]) => void; onToggle: (path: number[]) => void; depth?: number }) {
  const key = branchKey(root, node.path);
  const open = expanded.has(key);
  return <>
    <div className={"task-list-row child" + (key === selected ? " selected" : "")} style={{ paddingLeft: 8 + depth * 16 }}>
      {node.children.length ? <button className="task-disclosure" onClick={() => onToggle(node.path)} title={open ? "Collapse subtasks" : "Expand subtasks"} aria-label={open ? `Collapse ${node.name}` : `Expand ${node.name}`}>{open ? <Minus size={11} /> : <Plus size={11} />}</button> : <span className="task-disclosure-spacer" />}
      <button className="task-list-select" onClick={() => onSelect(node.path)} title={`Task ID ${node.id}`}>
        <span className="mono task-tree-id">{node.id}</span>
        <span className="truncate">{node.name || "(unnamed task)"}</span>
        {!!node.children.length && <span className="task-child-count">{node.children.length}</span>}
      </button>
    </div>
    {open && node.children.map((child) => <NestedTaskRow key={pathKey(child.path)} root={root} node={child} selected={selected} expanded={expanded} onSelect={onSelect} onToggle={onToggle} depth={depth + 1} />)}
  </>;
}

function FieldRow({ field, depth = 0 }: { field: TaskFieldView; depth?: number }) {
  const hasChildren = !!field.children?.length;
  if (hasChildren) {
    return <details className={"task-field-group" + (field.raw ? " raw" : "")} open={depth === 0 && field.name === "fixed"}>
      <summary className="task-field-row" style={{ paddingLeft: 12 + depth * 16 }}>
        <span className="task-field-name">{label(field.name)}</span>
        <span className="task-field-value muted">{field.value}</span>
        <span className="task-field-type mono">{field.ty}</span>
        <span className="task-field-offset mono">0x{field.offset.toString(16).toUpperCase().padStart(4, "0")}</span>
      </summary>
      {field.children!.map((child, index) => <FieldRow key={`${child.name}:${child.offset}:${index}`} field={child} depth={depth + 1} />)}
    </details>;
  }
  return <div className={"task-field-row leaf" + (field.raw ? " raw" : "")} style={{ paddingLeft: 28 + depth * 16 }} title={field.interpretation}>
    <span className="task-field-name">{label(field.name)}</span>
    <span className={"task-field-value" + (field.ty.includes("wstring") ? " text" : " mono")}>{field.value ?? ""}</span>
    <span className="task-field-type mono">{field.ty}</span>
    <span className="task-field-offset mono">0x{field.offset.toString(16).toUpperCase().padStart(4, "0")}</span>
    {field.interpretation && <span className="task-field-interpretation mono">{field.interpretation}</span>}
  </div>;
}

export const TasksEditor = forwardRef<TasksEditorHandle, Props>(function TasksEditor({ active, defaultPath, onStateChange }, ref) {
  const [file, setFile] = useState<TasksFileSummary | null>(null);
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query.trim().toLocaleLowerCase());
  const [page, setPage] = useState(0);
  const [pageInput, setPageInput] = useState("1");
  const [selectedRoot, setSelectedRoot] = useState<TaskRootSummary | null>(null);
  const [selectedPath, setSelectedPath] = useState<number[]>([]);
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [trees, setTrees] = useState<Map<string, TaskTreeNode>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [taskBusy, setTaskBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const autoOpened = useRef<string | null>(null);
  const request = useRef(0);

  const selectTask = useCallback(async (root: TaskRootSummary, taskPath: number[], expandAfter = false) => {
    const current = ++request.current;
    setSelectedRoot(root);
    setSelectedPath(taskPath);
    setTaskBusy(true);
    setError(null);
    try {
      const next = await getTask(root.pack, root.root, taskPath);
      if (request.current === current) {
        setDetail(next);
        setTrees((currentTrees) => new Map(currentTrees).set(rootKey(root), next.tree));
        if (expandAfter && next.tree.children.length) {
          setExpanded((currentExpanded) => new Set(currentExpanded).add(branchKey(root, [])));
        }
      }
    } catch (problem) {
      if (request.current === current) setError(String(problem).replace(/^Error: /, ""));
    } finally {
      if (request.current === current) setTaskBusy(false);
    }
  }, []);

  const load = useCallback(async (path: string) => {
    const current = ++request.current;
    setBusy(true);
    setTaskBusy(false);
    setError(null);
    setFile(null);
    setDetail(null);
    setSelectedRoot(null);
    setSelectedPath([]);
    setTrees(new Map());
    setExpanded(new Set());
    try {
      const opened = await openTasks(path);
      if (request.current !== current) return;
      setFile(opened);
      setQuery("");
      setPage(0);
      setBusy(false);
      if (opened.roots[0]) void selectTask(opened.roots[0], []);
    } catch (problem) {
      if (request.current === current) setError(String(problem).replace(/^Error: /, ""));
    } finally {
      if (request.current === current) setBusy(false);
    }
  }, [selectTask]);

  const choose = useCallback(async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      defaultPath: file?.path || defaultPath || undefined,
      title: "Open tasks.data",
      filters: [{ name: "tasks.data", extensions: ["data"] }],
    });
    if (typeof picked === "string") void load(picked);
  }, [defaultPath, file?.path, load]);

  useImperativeHandle(ref, () => ({ choose, openPath: (path) => void load(path) }), [choose, load]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey)) return;
      const key = event.key.toLowerCase();
      if (key !== "o" && key !== "s") return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (key === "o") void choose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, choose]);

  useEffect(() => {
    if (!active || !defaultPath || file || autoOpened.current === defaultPath) return;
    autoOpened.current = defaultPath;
    void load(defaultPath);
  }, [active, defaultPath, file, load]);

  useEffect(() => {
    onStateChange({ loaded: !!file, path: file?.path ?? null, summary: file });
  }, [file, onStateChange]);

  const filtered = useMemo(() => {
    if (!file || !deferredQuery) return file?.roots ?? [];
    const numeric = /^\d+$/.test(deferredQuery) ? Number(deferredQuery) : null;
    return file.roots.filter((root) => root.name.toLocaleLowerCase().includes(deferredQuery) || (numeric !== null ? root.id === numeric : String(root.id).includes(deferredQuery)));
  }, [deferredQuery, file]);
  const pages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const shown = filtered.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  useEffect(() => setPage(0), [deferredQuery]);
  useEffect(() => {
    const clamped = Math.min(page, pages - 1);
    if (clamped !== page) setPage(clamped);
    setPageInput(String(clamped + 1));
  }, [page, pages]);

  const applyPageInput = () => {
    const parsed = Number(pageInput);
    const next = Number.isFinite(parsed) ? Math.max(1, Math.min(pages, Math.trunc(parsed))) : page + 1;
    setPage(next - 1);
    setPageInput(String(next));
  };

  const toggleBranch = (root: TaskRootSummary, taskPath: number[]) => {
    const key = branchKey(root, taskPath);
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const selectedBranch = selectedRoot ? branchKey(selectedRoot, selectedPath) : null;

  if (!file) {
    return <section className="tasks-pane empty">
      <div className="drop-card tasks-empty">
        <ListTree size={34} />
        <h2>Open a tasks.data file</h2>
        <p className="muted">JD IDE verifies the index and every numbered task pack before showing the root-task list. Versions 165, 172 and 184 are supported.</p>
        <button className="btn primary" onClick={choose} disabled={busy}><FolderOpen size={15} /> {busy ? "Verifying task packs…" : "Choose tasks.data…"}</button>
        {defaultPath && <button className="btn" onClick={() => void load(defaultPath)} disabled={busy} title={defaultPath}>Open configured client tasks</button>}
        {error && <div className="path-data-message error">{error}</div>}
      </div>
    </section>;
  }

  return <section className="tasks-pane" aria-busy={busy || taskBusy}>
    <header className="tasks-head">
      <div>
        <h2>Tasks editor <span className="tag">read-only</span></h2>
        <div className="tasks-file-line">
          <span className="mono truncate" title={file.path}>{file.path}</span>
          <span className="path-data-badge"><b>Version:</b> v{file.version}</span>
          <span className="path-data-badge"><b>Roots:</b> {count(file.rootCount)}</span>
          <span className="path-data-badge"><b>Packs:</b> {file.packCount}</span>
          <span className="path-data-badge"><b>Size:</b> {bytes(file.size)}</span>
        </div>
      </div>
      <span className="tasks-integrity" title="The index, pack headers, offsets and every stored pack MD5 were verified"><ShieldCheck size={14} /> Integrity verified</span>
      <button className="btn" onClick={choose} disabled={busy}><FolderOpen size={14} /> Open…</button>
    </header>
    {error && <div className="path-data-message error">{error}</div>}
    <div className="tasks-body">
      <aside className="tasks-roots">
        <div className="tasks-search"><Search size={14} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search root tasks by ID or name…" autoComplete="off" /></div>
        <div className="tasks-root-list">
          {shown.map((root) => {
            const tree = trees.get(rootKey(root));
            const key = branchKey(root, []);
            const open = expanded.has(key);
            const selected = selectedBranch === key;
            return <div className="task-root-branch" key={rootKey(root)}>
              <div className={"task-list-row root" + (selected ? " selected" : "")}>
                {root.childCount ? <button className="task-disclosure" onClick={() => tree ? toggleBranch(root, []) : void selectTask(root, [], true)} title={open ? `Collapse ${root.childCount} subtasks` : `Expand ${root.childCount} subtasks`} aria-label={open ? `Collapse ${root.name}` : `Expand ${root.name}`}>{open ? <Minus size={11} /> : <Plus size={11} />}</button> : <span className="task-disclosure-spacer" />}
                <button className="task-list-select" onClick={() => void selectTask(root, [])} title={`Root task ${root.index + 1} · ID ${root.id}`}>
                  <span className="mono task-tree-id">{root.id}</span>
                  <span className="truncate">{root.name || "(unnamed task)"}</span>
                  <span className="muted mono">{bytes(root.byteSize)}</span>
                </button>
              </div>
              {open && tree?.children.map((child) => <NestedTaskRow key={pathKey(child.path)} root={root} node={child} selected={selectedBranch} expanded={expanded} onSelect={(taskPath) => void selectTask(root, taskPath)} onToggle={(taskPath) => toggleBranch(root, taskPath)} />)}
            </div>;
          })}
          {!shown.length && <div className="empty-note center">No root tasks match this search.</div>}
        </div>
        <footer className="tasks-page">
          <span>{filtered.length ? page * PAGE_SIZE + 1 : 0}–{Math.min((page + 1) * PAGE_SIZE, filtered.length)} of {count(filtered.length)}</span>
          <span className="spacer" />
          <button className="icon-btn small" onClick={() => setPage((value) => Math.max(0, value - 1))} disabled={page === 0} title="Previous page">‹</button>
          <label>Page <input type="number" min={1} max={pages} value={pageInput} onChange={(event) => setPageInput(event.target.value)} onBlur={applyPageInput} onKeyDown={(event) => event.key === "Enter" && applyPageInput()} /> / {pages}</label>
          <button className="icon-btn small" onClick={() => setPage((value) => Math.min(pages - 1, value + 1))} disabled={page + 1 >= pages} title="Next page">›</button>
        </footer>
      </aside>
      <section className="tasks-inspector">
        {detail ? <>
          <header className="tasks-inspector-head">
            <div className="task-title-icon"><FileCheck2 size={18} /></div>
            <div className="truncate"><h2 className="truncate">{detail.name || "(unnamed task)"}</h2><span className="muted mono">ID {detail.id} · root {selectedRoot ? selectedRoot.index + 1 : "?"} · offset 0x{detail.taskOffset.toString(16).toUpperCase()} · {bytes(detail.taskSize)}</span></div>
            {taskBusy && <span className="muted">Reading…</span>}
          </header>
          <div className="task-fields-head"><span>Field</span><span>Value</span><span>Type</span><span>Offset</span></div>
          <div className="task-fields">{detail.fields.map((field, index) => <FieldRow key={`${field.name}:${field.offset}:${index}`} field={field} />)}</div>
        </> : <div className="empty-note center">Select a task to inspect its fields.</div>}
      </section>
    </div>
  </section>;
});
