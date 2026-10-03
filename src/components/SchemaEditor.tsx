import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { getListSchema, previewListSchema, resetListSchema, saveListSchema, schemaContext } from "../elements/api";
import type { FileSummary, ListSchema, RecordDetail, SchemaContext } from "../elements/types";
import { count, hex } from "../elements/format";
import { type Path, allPaths, nodeAt } from "../elements/fieldPaths";
import {
  type Draft,
  type EditField,
  type Kind,
  type Row,
  KINDS,
  type EditRule,
  type FieldSpec,
  autoGroup,
  canHaveRules,
  formatValues,
  levels,
  parseValues,
  ruleControls,
  ruleKindsFor,
  uid,
  defineAt,
  groupUids,
  hasMembers,
  draftSize,
  fieldSize,
  fillTo,
  formatDims,
  fromDef,
  isPad,
  kindInfo,
  newField,
  hasDuplicates,
  parseDims,
  renameDuplicates,
  rows,
  toDef,
  validate,
} from "../schema/model";
import { FieldTree } from "./FieldTree";
import { ReadingsCard } from "./ReadingsCard";
import { ImportMenu } from "./ImportMenu";
import { PasteFieldsDialog } from "./PasteFieldsDialog";
import { RefsPicker } from "./RefsPicker";
import { isUndefinedNode, undefinedSpan } from "../elements/readings";
import { HexView } from "./HexView";
import { ArrowDown, ArrowDownToLine, ArrowUp, BetweenHorizontalEnd, Check, ChevronLeft, ChevronRight, ChevronsDownUp, ChevronsUpDown, Copy, Eraser, FolderOpen, GitBranch, Group, ListOrdered, ListPlus, Plus, Redo2, RotateCcw, Save, Trash2, Undo2, Ungroup, WandSparkles, X } from "lucide-react";

interface Props {
  summary: FileSummary;
  initialList: number;
  initialRow: number;
  /** A field to define when the editor opens (from the inspector's readings). */
  intent?: { list: number; offset: number; spec: FieldSpec } | null;
  /** Open the enums & masks editor (at a set, if given). */
  onEditSets?: (key: string | null) => void;
  onSaved: (summary: FileSummary) => void;
  onClose: (list: number) => void;
}

const INTEGER_KINDS = new Set<Kind>(["i8", "u8", "bool", "i16", "u16", "i32", "u32", "i64", "u64"]);
const ROLES = ["", "path", "icon", "skill", "buff", "money", "time", "duration", "duration_ms", "daytime"];
const ROLE_LABEL: Record<string, string> = {
  "": "—",
  path: "path",
  icon: "icon",
  skill: "skill (name from skillstr.txt)",
  buff: "buff (name from buff_str.txt)",
  money: "money (Gold / Silver / Copper)",
  time: "date/time (unix)",
  duration: "duration (seconds)",
  duration_ms: "duration (ms)",
  daytime: "time of day (seconds)",
};

// ---------------------------------------------------------------- tree edits

type Op = (siblings: EditField[], index: number) => EditField[];

function editTree(fields: EditField[], uid: number, op: Op): EditField[] {
  const i = fields.findIndex((f) => f.uid === uid);
  if (i >= 0) return op(fields, i);
  return fields.map((f) => (hasMembers(f.kind) ? { ...f, children: editTree(f.children, uid, op) } : f));
}

const patchOp =
  (patch: Partial<EditField>): Op =>
  (list, i) =>
    list.map((f, j) => (j === i ? { ...f, ...patch } : f));
const removeOp: Op = (list, i) => list.filter((_, j) => j !== i);
const moveOp =
  (delta: number): Op =>
  (list, i) => {
    const j = i + delta;
    if (j < 0 || j >= list.length) return list;
    const next = [...list];
    [next[i], next[j]] = [next[j], next[i]];
    return next;
  };
const ungroupOp: Op = (list, i) => [...list.slice(0, i), ...list[i].children, ...list.slice(i + 1)];

/** A name for a new group: the members' shared stem, e.g. "id_addon". */
function suggestGroupName(members: EditField[]): string {
  const names = members.filter((m) => !isPad(m)).map((m) => m.name.trim());
  if (!names.length) return "Group";
  let prefix = names[0];
  for (const n of names) while (!n.startsWith(prefix)) prefix = prefix.slice(0, -1);
  return prefix.replace(/[_\d]+$/, "") || "Group";
}

/** The changes a type switch brings: members for structs, a length for
 *  strings and bytes, and no enum/role/refs for non-integers. */
function kindPatch(f: EditField, kind: Kind): Partial<EditField> {
  const p: Partial<EditField> = { kind };
  if (kind === "struct" && !f.children.length) p.children = [newField({ name: "value" })];
  if (kindInfo(kind).len && !(f.len >= 1)) p.len = kind === "bytes" ? 4 : 32;
  if (!INTEGER_KINDS.has(kind)) Object.assign(p, { e: "", display: "", refs: [] });
  return p;
}

/** Uids of every field (not groups), nested ones included. */
function fieldUids(fields: EditField[], out: number[] = []): number[] {
  for (const f of fields) {
    if (f.kind !== "group") out.push(f.uid);
    fieldUids(f.children, out);
  }
  return out;
}

const insertAfterOp =
  (field: EditField): Op =>
  (list, i) =>
    [...list.slice(0, i + 1), field, ...list.slice(i + 1)];

// ---------------------------------------------------------------- component

/** Uids of the structs and groups containing `uid`. */
function ancestorsOf(fields: EditField[], uid: number, chain: number[] = []): number[] | null {
  for (const f of fields) {
    if (f.uid === uid) return chain;
    const found = ancestorsOf(f.children, uid, [...chain, f.uid]);
    if (found) return found;
  }
  return null;
}

export function SchemaEditor({ summary, initialList, initialRow, intent, onEditSets, onSaved, onClose }: Props) {
  const [list, setList] = useState(initialList);
  const [schema, setSchema] = useState<ListSchema | null>(null);
  const [context, setContext] = useState<SchemaContext | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tab, setTab] = useState<"fields" | "json">("fields");
  const [jsonText, setJsonText] = useState("");
  const [jsonError, setJsonError] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const [selected, setSelected] = useState<number | null>(null);
  // Rows picked for grouping: siblings of one level (click, Shift+click, Ctrl+click).
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const pickAnchor = useRef<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [pasting, setPasting] = useState(false);
  // Fields whose conditional type rules are shown.
  const [rulesOpen, setRulesOpen] = useState<Set<number>>(new Set());
  const [previewRow, setPreviewRow] = useState(initialRow);
  const [preview, setPreview] = useState<RecordDetail | null>(null);
  const [previewExpanded, setPreviewExpanded] = useState<Set<Path>>(new Set());
  const [previewHover, setPreviewHover] = useState<Path | null>(null);
  const [previewSelected, setPreviewSelected] = useState<Path | null>(null);
  const [previewRead, setPreviewRead] = useState<number | null>(null);
  const pendingIntent = useRef(intent ?? null);
  const undo = useRef<{ past: Draft[]; future: Draft[]; lastTag: string; lastAt: number }>({
    past: [],
    future: [],
    lastTag: "",
    lastAt: 0,
  });

  const listSummary = summary.lists[list];

  // Load the list's current definition.
  const load = useCallback(async (index: number) => {
    setError(null);
    const result = await getListSchema(index);
    setSchema(result);
    const fresh = fromDef(result.def, `List ${index}`);
    setDraft(fresh);
    setDirty(false);
    setSelected(null);
    setPicked(new Set());
    pickAnchor.current = null;
    setPreviewRead(null);
    setPreviewSelected(null);
    // Groups start collapsed.
    setCollapsed(new Set(groupUids(fresh.fields)));
    // Opened from the inspector to define a field: place it right away.
    const pending = pendingIntent.current;
    if (pending && pending.list === index) {
      pendingIntent.current = null;
      const placed = defineAt(fresh.fields, result.itemSize, pending.offset, pending.spec);
      if ("error" in placed) {
        setError(placed.error);
      } else {
        setDraft({ ...fresh, fields: placed.fields });
        setDirty(true);
        setSelected(placed.uid);
        setPicked(new Set([placed.uid]));
        const open = ancestorsOf(placed.fields, placed.uid) ?? [];
        setCollapsed(new Set(groupUids(placed.fields).filter((u) => !open.includes(u))));
      }
    }
    setPreviewExpanded(new Set());
    undo.current = { past: [], future: [], lastTag: "", lastAt: 0 };
  }, []);

  useEffect(() => {
    load(list).catch((e) => setError(String(e)));
  }, [list, load]);

  useEffect(() => {
    schemaContext()
      .then(setContext)
      .catch((e) => setError(String(e)));
  }, [summary]);

  /** Applies a change, coalescing rapid edits of the same thing for undo. */
  const commit = useCallback((next: Draft, tag: string) => {
    setDraft((prev) => {
      if (prev) {
        const u = undo.current;
        const now = Date.now();
        if (tag !== u.lastTag || now - u.lastAt > 1000) u.past = [...u.past.slice(-99), prev];
        u.future = [];
        u.lastTag = tag;
        u.lastAt = now;
      }
      return next;
    });
    setDirty(true);
    setNotice(null);
  }, []);

  const editField = (uid: number, op: Op, tag: string) => draft && commit({ ...draft, fields: editTree(draft.fields, uid, op) }, tag);
  const patch = (uid: number, p: Partial<EditField>, prop: string) => editField(uid, patchOp(p), `${uid}:${prop}`);

  const undoRedo = useCallback((direction: "undo" | "redo") => {
    const u = undo.current;
    setDraft((current) => {
      if (!current) return current;
      const from = direction === "undo" ? u.past : u.future;
      const target = from.at(-1);
      if (!target) return current;
      if (direction === "undo") {
        u.past = u.past.slice(0, -1);
        u.future = [...u.future, current];
      } else {
        u.future = u.future.slice(0, -1);
        u.past = [...u.past, current];
      }
      u.lastTag = "";
      return target;
    });
    setDirty(true);
  }, []);

  const problems = useMemo(() => (draft ? validate(draft) : new Map()), [draft]);
  const size = draft ? draftSize(draft.fields) : 0;
  const itemSize = schema?.itemSize ?? listSummary.itemSize;
  const def = useMemo(() => (draft ? toDef(draft) : null), [draft]);

  // Live preview of the draft on one record.
  useEffect(() => {
    if (!def || !schema || problems.size || schema.count === 0) {
      setPreview(null);
      return;
    }
    const row = Math.min(previewRow, schema.count - 1);
    let cancelled = false;
    const timer = setTimeout(() => {
      previewListSchema(list, row, def)
        .then((r) => !cancelled && setPreview(r))
        .catch((e) => !cancelled && setError(String(e)));
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [def, list, previewRow, schema, problems]);

  const confirmDiscard = () => !dirty || window.confirm("Discard your unsaved changes to this list?");

  const switchList = (index: number) => {
    if (index === list || !confirmDiscard()) return;
    setPreviewRow(0);
    setList(index);
  };

  const close = () => confirmDiscard() && onClose(list);

  const save = async () => {
    if (!def || problems.size || size > itemSize) return;
    setBusy(true);
    setError(null);
    try {
      onSaved(await saveListSchema(list, def));
      await load(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const revert = async () => {
    if (!schema) return;
    const what = schema.hasBuiltin ? "the built-in definition" : "no definition (borrowed or unknown)";
    if (!window.confirm(`Remove your schema for this list and go back to ${what}?`)) return;
    setBusy(true);
    try {
      onSaved(await resetListSchema(list));
      await load(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // Keep the selected row in view (e.g. a field just defined from a reading).
  useEffect(() => {
    if (selected !== null) document.querySelector(`[data-uid="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected, collapsed]);

  // Keyboard: Ctrl+S save, Ctrl+Z / Ctrl+Y undo/redo, Esc close.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      const typing = e.target instanceof HTMLTextAreaElement;
      if (mod && e.key.toLowerCase() === "s") {
        e.preventDefault();
        save();
      } else if (mod && !typing && e.key.toLowerCase() === "z") {
        e.preventDefault();
        undoRedo(e.shiftKey ? "redo" : "undo");
      } else if (mod && !typing && e.key.toLowerCase() === "y") {
        e.preventDefault();
        undoRedo("redo");
      } else if (e.key === "Escape" && !(e.target instanceof HTMLInputElement || typing)) {
        close();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (!draft || !schema) {
    return (
      <div className="schema-editor">
        <div className="empty-note center">{error ?? "Loading…"}</div>
      </div>
    );
  }

  const tableRows = rows(draft.fields, collapsed);
  const levelOf = levels(draft.fields);
  const selectedRow = tableRows.find((r) => r.field.uid === selected);
  const hoveredNode = previewHover && preview ? nodeAt(preview.nodes, previewHover) : null;
  const highlight = hoveredNode
    ? { off: hoveredNode.off, size: hoveredNode.size }
    : selectedRow
      ? { off: selectedRow.off, size: fieldSize(selectedRow.field) }
      : null;

  const toggleCollapsed = (uid: number) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (next.has(uid)) next.delete(uid);
      else next.add(uid);
      return next;
    });

  const toggleRules = (uid: number) =>
    setRulesOpen((open) => {
      const next = new Set(open);
      if (next.has(uid)) next.delete(uid);
      else next.add(uid);
      return next;
    });

  const togglePick = (uid: number) => {
    setPicked((p) => {
      const next = new Set(p);
      if (next.has(uid)) next.delete(uid);
      else next.add(uid);
      // A group stays picked only while all its fields are.
      const walk = (fields: EditField[]) => {
        for (const g of fields) {
          if (g.kind === "group" && next.has(g.uid) && !fieldUids(g.children).every((u) => next.has(u))) next.delete(g.uid);
          walk(g.children);
        }
      };
      walk(draft.fields);
      return next;
    });
    pickAnchor.current = uid;
  };

  /** Whether a group's fields are picked: all, some or none. */
  const groupState = (group: EditField): "all" | "some" | "none" => {
    const members = fieldUids(group.children);
    const n = members.filter((u) => picked.has(u)).length;
    return n === 0 ? "none" : n === members.length ? "all" : "some";
  };

  /** A group's checkbox picks (or unpicks) the group with every field in it. */
  const toggleGroupPick = (group: EditField) => {
    const members = fieldUids(group.children);
    const all = members.length > 0 && members.every((u) => picked.has(u));
    setPicked((p) => {
      const next = new Set(p);
      for (const u of [group.uid, ...members]) {
        if (all) next.delete(u);
        else next.add(u);
      }
      return next;
    });
    pickAnchor.current = group.uid;
  };

  const clickRow = (e: React.MouseEvent, row: Row) => {
    const uid = row.field.uid;
    setSelected(uid);
    // Editing a cell keeps the current selection.
    const interactive = (e.target as HTMLElement).closest("input, select, textarea, button, .refs-chips");
    if (interactive && !e.shiftKey && !e.ctrlKey && !e.metaKey) return;
    const sameLevel = (other: number) => row.parent.some((f) => f.uid === other);
    if (e.shiftKey && pickAnchor.current !== null && sameLevel(pickAnchor.current)) {
      const a = row.parent.findIndex((f) => f.uid === pickAnchor.current);
      const b = row.parent.indexOf(row.field);
      setPicked(new Set(row.parent.slice(Math.min(a, b), Math.max(a, b) + 1).map((f) => f.uid)));
      return;
    }
    if ((e.ctrlKey || e.metaKey) && [...picked].every(sameLevel)) {
      setPicked((p) => {
        const next = new Set(p);
        if (next.has(uid)) next.delete(uid);
        else next.add(uid);
        return next;
      });
      pickAnchor.current = uid;
      return;
    }
    pickAnchor.current = uid;
    setPicked(new Set([uid]));
  };

  const pickedRows = tableRows.filter((r) => picked.has(r.field.uid));
  const canGroup =
    pickedRows.length > 0 &&
    pickedRows.every((r) => r.parent === pickedRows[0].parent && r.field.kind !== "group") &&
    pickedRows[0].owner?.kind !== "group";

  const groupPicked = () => {
    if (!canGroup) return;
    const parent = pickedRows[0].parent;
    const indexes = pickedRows.map((r) => parent.indexOf(r.field));
    const lo = Math.min(...indexes);
    const hi = Math.max(...indexes);
    const members = parent.slice(lo, hi + 1);
    const group = newField({ name: suggestGroupName(members), kind: "group", children: members });
    editField(parent[lo].uid, (list, i) => [...list.slice(0, i), group, ...list.slice(i + members.length)], "group");
    setCollapsed((c) => new Set([...c, group.uid]));
    setSelected(group.uid);
    setPicked(new Set([group.uid]));
    pickAnchor.current = group.uid;
  };

  // Groups to ungroup: the picked ones, else the selected row's group.
  const groupsToUngroup = (() => {
    const pickedGroups = tableRows.filter((r) => picked.has(r.field.uid) && r.field.kind === "group").map((r) => r.field.uid);
    if (pickedGroups.length) return pickedGroups;
    const row = tableRows.find((r) => r.field.uid === selected);
    if (row?.field.kind === "group") return [row.field.uid];
    if (row?.owner?.kind === "group") return [row.owner.uid];
    return [];
  })();

  /** Puts the fields of the chosen groups back in place (they keep names and offsets). */
  const ungroupChosen = () => {
    if (!groupsToUngroup.length) return;
    const chosen = new Set(groupsToUngroup);
    const walk = (fields: EditField[]): EditField[] =>
      fields.flatMap((f) => (chosen.has(f.uid) ? walk(f.children) : [f.children.length ? { ...f, children: walk(f.children) } : f]));
    commit({ ...draft, fields: walk(draft.fields) }, "ungroup");
    setPicked(new Set());
  };

  // ------------------------------------------------------------ bulk edit

  const allFields = fieldUids(draft.fields);
  const pickedFields = allFields.filter((u) => picked.has(u));
  const allPicked = allFields.length > 0 && pickedFields.length === allFields.length;

  /** Applies a change to every picked field (one undo step). */
  const applyBulk = (change: (f: EditField) => Partial<EditField> | null, tag: string) => {
    const walk = (fields: EditField[]): EditField[] =>
      fields.map((f) => {
        const p = picked.has(f.uid) && f.kind !== "group" ? change(f) : null;
        const next = p ? { ...f, ...p } : f;
        return next.children.length ? { ...next, children: walk(next.children) } : next;
      });
    commit({ ...draft, fields: walk(draft.fields) }, tag);
  };

  const deletePicked = () => {
    if (!window.confirm(`Remove the ${picked.size} selected rows?`)) return;
    const walk = (fields: EditField[]): EditField[] =>
      fields.filter((f) => !picked.has(f.uid)).map((f) => (f.children.length ? { ...f, children: walk(f.children) } : f));
    commit({ ...draft, fields: walk(draft.fields) }, "bulk-delete");
    setPicked(new Set());
  };

  const autoGroupAll = () => {
    const created: number[] = [];
    const fields = autoGroup(draft.fields, created);
    if (!created.length) {
      setNotice("No runs of 3 or more numbered fields (like addon1, addon2, addon3) to group.");
      return;
    }
    commit({ ...draft, fields }, "autogroup");
    setCollapsed((c) => new Set([...c, ...created]));
    setNotice(`Created ${created.length} group${created.length > 1 ? "s" : ""}.`);
  };

  /** Defines a field from a preview reading, directly in the draft. */
  const defineFromPreview = (offset: number, spec: FieldSpec) => {
    const placed = defineAt(draft.fields, itemSize, offset, spec);
    if ("error" in placed) {
      setError(placed.error);
      return;
    }
    commit({ ...draft, fields: placed.fields }, "define");
    setSelected(placed.uid);
    setPicked(new Set([placed.uid]));
    setPreviewRead(null);
    const open = ancestorsOf(placed.fields, placed.uid) ?? [];
    setCollapsed((c) => new Set([...c].filter((u) => !open.includes(u))));
  };

  const previewSpan = preview && previewRead !== null ? undefinedSpan(preview.nodes, previewRead) : null;

  const addField = () => {
    const field = newField({ name: `field_${draft.fields.length + 1}` });
    const anchor = selectedRow?.field;
    if (anchor) editField(anchor.uid, insertAfterOp(field), "add");
    else commit({ ...draft, fields: [...draft.fields, field] }, "add");
    setSelected(field.uid);
  };

  const applyJson = () => {
    try {
      const parsed = JSON.parse(jsonText);
      if (typeof parsed !== "object" || parsed === null || typeof parsed.name !== "string") {
        throw new Error('Expected an object like { "name": "...", "struct": "...", "size": 0, "fields": [...] }');
      }
      if (parsed.fields !== undefined && !Array.isArray(parsed.fields)) throw new Error('"fields" must be an array');
      commit(fromDef(parsed, parsed.name), "json");
      setJsonError(null);
      setTab("fields");
    } catch (e) {
      setJsonError(e instanceof Error ? e.message : String(e));
    }
  };

  const sizeState = size === itemSize ? "ok" : size < itemSize ? "short" : "over";
  const canSave = dirty && !busy && problems.size === 0 && size <= itemSize;

  return (
    <div className="schema-editor">
      <header className="se-head">
        <div className="se-title">
          <h2>Schema editor</h2>
          <select
            className="se-list-select"
            value={list}
            onChange={(e) => switchList(Number(e.target.value))}
            title="List to edit"
          >
            {summary.lists.map((l) => (
              <option key={l.index} value={l.index}>
                {l.index} · {l.name} ({l.itemSize} B{l.custom ? ", edited" : ""})
              </option>
            ))}
          </select>
        </div>
        <div className={`se-meter ${sizeState}`} title="Bytes the schema describes / record size">
          <div className="se-meter-bar">
            <span style={{ width: `${Math.min(100, (size / Math.max(1, itemSize)) * 100)}%` }} />
          </div>
          <span className="mono">
            {size} / {itemSize} B
          </span>
          <span className="se-meter-note">
            {sizeState === "ok" ? "covers the record" : sizeState === "short" ? `${itemSize - size} B left` : `${size - itemSize} B too many`}
          </span>
        </div>
        <span className="spacer" />
        {schema.custom && (
          <button className="btn" onClick={revert} disabled={busy} title="Remove your edits for this list">
            <RotateCcw size={14} /> Revert to built-in
          </button>
        )}
        <button className="btn" onClick={() => load(list)} disabled={!dirty || busy} title="Throw away unsaved changes">
          <Eraser size={14} /> Discard
        </button>
        <button className="btn primary" onClick={save} disabled={!canSave} title="Save (Ctrl+S)">
          <Save size={14} /> {busy ? "Saving…" : "Save"}
        </button>
        <button className="icon-btn" onClick={close} title="Close the schema editor (Esc)" aria-label="Close">
          <X size={18} />
        </button>
      </header>

      <div className="se-sub">
        <span>
          Saves to layout <b className="mono">{schema.targetLayout}</b>
          {schema.targetExists ? "" : " (new, for this version)"}
          {schema.custom ? " · this list has your edits" : ""}
        </span>
        {schema.defLayout && schema.defLayout !== schema.targetLayout && (
          <span className="muted">Starting from the definition borrowed from {schema.defLayout}.</span>
        )}
        {!schema.def && <span className="muted">No definition is known for this list yet.</span>}
        <span className="muted">
          {count(schema.count)} records × {itemSize} B
        </span>
      </div>

      {error && (
        <div className="error-bar" role="alert">
          <span>{error}</span>
          <button className="link" onClick={() => setError(null)}>
            Dismiss
          </button>
        </div>
      )}

      <div className="se-body">
        <section className="se-fields">
          <div className="se-meta">
            <label>
              <span>List name</span>
              <input
                className={"se-input" + (problems.has("") ? " invalid" : "")}
                value={draft.name}
                onChange={(e) => commit({ ...draft, name: e.target.value }, "name")}
                spellCheck={false}
              />
            </label>
            <label>
              <span>Struct</span>
              <input
                className="se-input mono"
                value={draft.struct}
                placeholder="e.g. EQUIPMENT_ESSENCE"
                onChange={(e) => commit({ ...draft, struct: e.target.value }, "struct")}
                spellCheck={false}
                title="Other lists' references point at lists by struct name"
              />
            </label>
            <div className="tabs" role="tablist">
              <button className={tab === "fields" ? "active" : ""} onClick={() => setTab("fields")}>
                Fields
              </button>
              <button
                className={tab === "json" ? "active" : ""}
                onClick={() => {
                  setJsonText(JSON.stringify(toDef(draft), null, 2));
                  setJsonError(null);
                  setTab("json");
                }}
              >
                JSON
              </button>
            </div>
          </div>

          {tab === "fields" ? (
            <>
              <div className="se-toolbar">
                <ImportMenu
                  list={list}
                  onImport={(def, from) => {
                    const imported = fromDef(def, def.name);
                    commit(imported, "import");
                    setCollapsed(new Set(groupUids(imported.fields)));
                    setSelected(null);
                    setPicked(new Set());
                    setNotice(`Imported from ${from}. Review it, then save.`);
                  }}
                  onPaste={() => setPasting(true)}
                />
                {pasting && draft && (
                  <PasteFieldsDialog
                    list={list}
                    listName={listSummary.name}
                    itemSize={itemSize}
                    onCancel={() => setPasting(false)}
                    onApply={(fields, struct) => {
                      setPasting(false);
                      commit({ ...draft, fields, struct: draft.struct || struct || "" }, "import");
                      setCollapsed(new Set());
                      setSelected(null);
                      setPicked(new Set());
                      setNotice(`${fields.length} fields from the pasted list. Review them, then save.`);
                    }}
                  />
                )}
                <button className="btn small" onClick={addField}>
                  <Plus size={14} /> Field
                </button>
                <button
                  className="btn small"
                  onClick={() => commit({ ...draft, fields: fillTo(draft.fields, itemSize) }, "fill")}
                  disabled={size >= itemSize}
                  title="Append int32 fields named unknown_XXXX up to the record size"
                >
                  <ArrowDownToLine size={14} /> Fill to {itemSize} B
                </button>
                <span className="se-sep" />
                <button
                  className="btn small"
                  onClick={groupPicked}
                  disabled={!canGroup}
                  title="Put the picked rows (and any between them) into a collapsible group. Shift+click picks a range, Ctrl+click adds rows."
                >
                  <Group size={14} /> Group{pickedRows.length > 1 ? ` ${pickedRows.length} rows` : ""}
                </button>
                <button
                  className="btn small"
                  onClick={ungroupChosen}
                  disabled={!groupsToUngroup.length}
                  title={groupsToUngroup.length ? "Put the fields of the selected group(s) back in place; they keep their names and offsets" : "Select a group (or a field in one) to ungroup it"}
                >
                  <Ungroup size={14} /> Ungroup{groupsToUngroup.length > 1 ? ` ${groupsToUngroup.length}` : ""}
                </button>
                <button
                  className="btn small"
                  onClick={autoGroupAll}
                  title="Group runs of numbered fields such as id_addon1…id_addon5"
                >
                  <WandSparkles size={14} /> Auto-group numbered
                </button>
                {onEditSets && (
                  <button className="btn small" onClick={() => onEditSets(null)} title="Edit the shared enums and masks">
                    <ListOrdered size={14} /> Enums &amp; masks
                  </button>
                )}
                {notice && <span className="muted small">{notice}</span>}
                <span className="spacer" />
                <button className="link" onClick={() => undoRedo("undo")} disabled={!undo.current.past.length}>
                  <Undo2 size={13} /> Undo
                </button>
                <button className="link" onClick={() => undoRedo("redo")} disabled={!undo.current.future.length}>
                  <Redo2 size={13} /> Redo
                </button>
                <button className="link" onClick={() => setCollapsed(new Set())}>
                  <ChevronsUpDown size={13} /> Expand all
                </button>
                <button
                  className="link"
                  onClick={() => setCollapsed(new Set(tableRows.filter((r) => hasMembers(r.field.kind)).map((r) => r.field.uid).concat(groupUids(draft.fields))))}
                >
                  <ChevronsDownUp size={13} /> Collapse all
                </button>
              </div>
              {picked.size > 1 && (
                <div className="se-bulk" role="toolbar" aria-label="Bulk edit">
                  <span className="se-bulk-count">
                    <b>{picked.size}</b> selected
                  </span>
                  <label>
                    Type
                    <select
                      className="cell"
                      value=""
                      onChange={(e) => e.target.value && applyBulk((f) => kindPatch(f, e.target.value as Kind), "bulk-kind")}
                    >
                      <option value="">set…</option>
                      {KINDS.map((k) => (
                        <option key={k.kind} value={k.kind}>
                          {k.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label title="Applies to integer fields only">
                    Enum / mask
                    <select
                      className="cell"
                      value=""
                      onChange={(e) => {
                        const v = e.target.value;
                        if (v === "__edit__") onEditSets?.(null);
                        else if (v) applyBulk((f) => (INTEGER_KINDS.has(f.kind) ? { e: v === "__none__" ? "" : v } : null), "bulk-e");
                      }}
                    >
                      <option value="">set…</option>
                      <option value="__none__">— none —</option>
                      {context?.enums.map((x) => (
                        <option key={x.key} value={x.key}>
                          {x.label}
                          {x.flags ? " (mask)" : ""}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label title="Applies to integer fields only">
                    Role
                    <select
                      className="cell"
                      value=""
                      onChange={(e) => {
                        const v = e.target.value;
                        if (v) applyBulk((f) => (INTEGER_KINDS.has(f.kind) ? { display: v === "__none__" ? "" : v } : null), "bulk-role");
                      }}
                    >
                      <option value="">set…</option>
                      <option value="__none__">— none —</option>
                      {ROLES.filter(Boolean).map((r) => (
                        <option key={r} value={r}>
                          {ROLE_LABEL[r]}
                        </option>
                      ))}
                    </select>
                  </label>
                  <button className="btn small" onClick={() => applyBulk((f) => (f.refs.length ? { refs: [] } : null), "bulk-refs")}>
                    Clear refs
                  </button>
                  <button className="btn small" onClick={groupPicked} disabled={!canGroup} title={canGroup ? "Group the selected rows" : "Only rows of one level can be grouped"}>
                    <Group size={14} /> Group
                  </button>
                  {groupsToUngroup.length > 0 && (
                    <button className="btn small" onClick={ungroupChosen} title="Put the fields of the selected groups back in place">
                      <Ungroup size={14} /> Ungroup {groupsToUngroup.length}
                    </button>
                  )}
                  <button className="btn small danger-btn" onClick={deletePicked}>
                    <Trash2 size={14} /> Delete
                  </button>
                  <span className="spacer" />
                  <button className="link" onClick={() => setPicked(new Set())}>
                    Clear selection
                  </button>
                </div>
              )}
              <div className="se-table scroll">
                <div className="se-grid se-grid-head">
                  <span className="se-head-field">
                    <input
                      type="checkbox"
                      className="row-check"
                      checked={allPicked}
                      ref={(el) => {
                        if (el) el.indeterminate = pickedFields.length > 0 && !allPicked;
                      }}
                      onChange={() => setPicked(allPicked ? new Set() : new Set(allFields))}
                      title={allPicked ? "Clear the selection" : "Select every field"}
                      aria-label="Select all fields"
                    />
                    Field
                  </span>
                  <span>Type</span>
                  <span>Length</span>
                  <span>Array</span>
                  <span>Enum / mask</span>
                  <span>Role</span>
                  <span>Refs</span>
                  <span>Comment</span>
                  <span>Offset</span>
                  <span>Size</span>
                  <span />
                </div>
                {tableRows.length === 0 && (
                  <div className="empty-note">
                    No fields yet. Add one, or use “Fill to {itemSize} B” to cover the record with int32 values and rename
                    them as you work out what they are.
                  </div>
                )}
                {tableRows.map((row) => {
                  const { field: f, depth, off, parent } = row;
                  const info = kindInfo(f.kind);
                  const integer = INTEGER_KINDS.has(f.kind);
                  const problem = problems.get(f.uid);
                  const index = parent.indexOf(f);
                  const pad = isPad(f);
                  const rowClass =
                    "se-grid se-row" +
                    (f.uid === selected ? " active" : "") +
                    (picked.has(f.uid) && picked.size > 1 ? " picked" : "") +
                    (problem ? " has-problem" : "") +
                    (pad ? " pad" : "");
                  if (f.kind === "group") {
                    const open = !collapsed.has(f.uid);
                    const members = f.children.filter((c) => !isPad(c)).length;
                    return (
                      <div
                        key={f.uid}
                        data-uid={f.uid}
                        className={rowClass + " se-group-row"}
                        onClick={(e) => clickRow(e, row)}
                      >
                        <span className="se-name">
                          <input
                            type="checkbox"
                            className="row-check"
                            checked={groupState(f) === "all"}
                            ref={(el) => {
                              if (el) el.indeterminate = groupState(f) === "some";
                            }}
                            onClick={(e) => e.stopPropagation()}
                            onChange={() => toggleGroupPick(f)}
                            aria-label={`Select ${f.name} and its fields`}
                            title="Select the group and every field in it"
                          />
                          <span className="indent" style={{ width: depth * 16 }} />
                          <button
                            className={"caret" + (open ? " open" : "")}
                            onClick={(e) => {
                              e.stopPropagation();
                              toggleCollapsed(f.uid);
                            }}
                            aria-label={open ? "Collapse group" : "Expand group"}
                          >
                            <ChevronRight size={15} />
                          </button>
                          <input
                            className={"cell se-group-name" + (problem ? " invalid" : "")}
                            value={f.name}
                            onChange={(e) => patch(f.uid, { name: e.target.value }, "name")}
                            spellCheck={false}
                            title={problem ?? "Group name (display only, fields keep their names and offsets)"}
                          />
                        </span>
                        <span className="se-group-info" style={{ gridColumn: "2 / 9" }}>
                          <span className="se-group-tag">group</span>
                          {members} field{members === 1 ? "" : "s"}
                          {!open && (
                            <span className="muted">
                              {" "}
                              · {f.children
                                .filter((c) => !isPad(c))
                                .slice(0, 4)
                                .map((c) => c.name)
                                .join(", ")}
                              {members > 4 ? ", …" : ""}
                            </span>
                          )}
                        </span>
                        <span className="mono muted cell-static">{hex(off, 4)}</span>
                        <span className="mono muted cell-static">{fieldSize(f)}</span>
                        <span className="se-actions">
                          <button title="Move up" onClick={() => editField(f.uid, moveOp(-1), "move")} disabled={index === 0}>
                            <ArrowUp size={14} />
                          </button>
                          <button
                            title="Move down"
                            onClick={() => editField(f.uid, moveOp(1), "move")}
                            disabled={index === parent.length - 1}
                          >
                            <ArrowDown size={14} />
                          </button>
                          <button title="Ungroup (keep the fields)" onClick={() => editField(f.uid, ungroupOp, "ungroup")}>
                            <Ungroup size={14} />
                          </button>
                          <button
                            title="Remove the group and its fields"
                            className="danger"
                            onClick={() =>
                              window.confirm(`Remove the group "${f.name}" and its ${members} fields?`) &&
                              editField(f.uid, removeOp, "remove")
                            }
                          >
                            <Trash2 size={14} />
                          </button>
                        </span>
                      </div>
                    );
                  }
                  return (
                    <Fragment key={f.uid}>
                    <div
                      className={rowClass}
                      onClick={(e) => clickRow(e, row)}
                      data-uid={f.uid}
                      onFocus={() => setSelected(f.uid)}
                    >
                      <span className="se-name">
                        <input
                            type="checkbox"
                            className="row-check"
                            checked={picked.has(f.uid)}
                            onClick={(e) => e.stopPropagation()}
                            onChange={() => togglePick(f.uid)}
                            aria-label={`Select ${f.name}`}
                          />
                          <span className="indent" style={{ width: depth * 16 }} />
                        {f.kind === "struct" ? (
                          <button
                            className={"caret" + (collapsed.has(f.uid) ? "" : " open")}
                            onClick={(e) => {
                              e.stopPropagation();
                              toggleCollapsed(f.uid);
                            }}
                            aria-label="Toggle members"
                          >
                            <ChevronRight size={15} />
                          </button>
                        ) : (
                          <span className="caret-space" />
                        )}
                        <input
                          className={"cell mono" + (problem ? " invalid" : "")}
                          value={f.name}
                          onChange={(e) => patch(f.uid, { name: e.target.value }, "name")}
                          spellCheck={false}
                          title={problem ?? (pad ? "Padding: skipped bytes, not saved as a field" : undefined)}
                        />
                        {f.when.length > 0 && (
                          <button
                            className="rule-chip"
                            title={`${f.when.length} conditional type rule${f.when.length > 1 ? "s" : ""}. Click to edit.`}
                            onClick={(e) => {
                              e.stopPropagation();
                              toggleRules(f.uid);
                            }}
                          >
                            <GitBranch size={11} /> {f.when.length}
                          </button>
                        )}
                      </span>
                      <select
                        className="cell"
                        value={f.kind}
                        onChange={(e) => {
                          patch(f.uid, kindPatch(f, e.target.value as Kind), "kind");
                        }}
                      >
                        {KINDS.map((k) => (
                          <option key={k.kind} value={k.kind}>
                            {k.label}
                          </option>
                        ))}
                      </select>
                      {info.len ? (
                        <span
                          className="len-cell"
                          title={
                            info.len === "chars"
                              ? `${f.len || 0} characters = ${(f.len || 0) * 2} bytes (UTF-16, 2 bytes each). Tools that write wstring:N count bytes.`
                              : `${f.len || 0} bytes`
                          }
                        >
                          <input
                            className="cell mono num"
                            type="number"
                            min={1}
                            value={f.len || ""}
                            onChange={(e) => patch(f.uid, { len: Math.max(0, Math.floor(Number(e.target.value))) }, "len")}
                          />
                          <span className="len-unit">{info.len === "chars" ? "ch" : "B"}</span>
                        </span>
                      ) : (
                        <span className="muted cell-static">—</span>
                      )}
                      <DimsInput value={f.dims} onChange={(dims) => patch(f.uid, { dims }, "dims")} />
                      {!integer ? (
                        <span className="muted cell-static">—</span>
                      ) : (
                      <select
                        className={"cell" + (f.e ? " has-value" : "")}
                        value={f.e}
                        onChange={(e) => {
                          if (e.target.value === "__edit__") onEditSets?.(f.e || null);
                          else patch(f.uid, { e: e.target.value }, "e");
                        }}
                      >
                        <option value="">—</option>
                        {f.e && !context?.enums.some((x) => x.key === f.e) && <option value={f.e}>{f.e}</option>}
                        {context?.enums.map((x) => (
                          <option key={x.key} value={x.key}>
                            {x.label}
                            {x.flags ? " (mask)" : ""}
                          </option>
                        ))}
                        {onEditSets && <option value="__edit__">Edit enums &amp; masks…</option>}
                      </select>
                      )}
                      {!integer ? (
                        <span className="muted cell-static">—</span>
                      ) : (
                        <select
                          className={"cell" + (f.display ? " has-value" : "")}
                          value={f.display}
                          onChange={(e) => patch(f.uid, { display: e.target.value }, "display")}
                        >
                          {ROLES.map((r) => (
                            <option key={r} value={r}>
                              {ROLE_LABEL[r] ?? r}
                            </option>
                          ))}
                        </select>
                      )}
                      <RefsPicker
                        value={f.refs}
                        targets={context?.targets ?? []}
                        disabled={!integer}
                        onChange={(refs) => patch(f.uid, { refs }, "refs")}
                      />
                      <input
                        className="cell"
                        value={f.c}
                        placeholder=""
                        onChange={(e) => patch(f.uid, { c: e.target.value }, "c")}
                        title={f.c}
                      />
                      <span className="mono muted cell-static">{hex(off, 4)}</span>
                      <span className="mono muted cell-static">{fieldSize(f)}</span>
                      <span className="se-actions">
                        <button title="Move up" onClick={() => editField(f.uid, moveOp(-1), "move")} disabled={index === 0}>
                          <ArrowUp size={14} />
                        </button>
                        <button
                          title="Move down"
                          onClick={() => editField(f.uid, moveOp(1), "move")}
                          disabled={index === parent.length - 1}
                        >
                          <ArrowDown size={14} />
                        </button>
                        {f.kind === "struct" && (
                          <button
                            title="Add a member"
                            onClick={() =>
                              patch(f.uid, { children: [...f.children, newField({ name: `m${f.children.length + 1}` })] }, "child")
                            }
                          >
                            <ListPlus size={14} />
                          </button>
                        )}
                        <button
                          title="Insert a field below"
                          onClick={() => {
                            const field = newField({ name: `field_${parent.length + 1}` });
                            editField(f.uid, insertAfterOp(field), "add");
                            setSelected(field.uid);
                          }}
                        >
                          <BetweenHorizontalEnd size={14} />
                        </button>
                        {canHaveRules(f) && (
                          <button
                            title="Conditional type: read this field as another type depending on a field next to it"
                            className={f.when.length || rulesOpen.has(f.uid) ? "on" : ""}
                            onClick={() => toggleRules(f.uid)}
                          >
                            <GitBranch size={14} />
                          </button>
                        )}
                        <button title="Remove" className="danger" onClick={() => editField(f.uid, removeOp, "remove")}>
                          <Trash2 size={14} />
                        </button>
                      </span>
                    </div>
                    {rulesOpen.has(f.uid) && canHaveRules(f) && (
                      <RulesPanel
                        field={f}
                        depth={depth}
                        controls={ruleControls(f, levelOf.get(f.uid) ?? [])}
                        onChange={(when) => patch(f.uid, { when }, "when")}
                      />
                    )}
                    </Fragment>
                  );
                })}
              </div>
              {problems.size > 0 && (
                <div className="se-problems">
                  {problems.size} problem{problems.size > 1 ? "s" : ""} to fix before saving:{" "}
                  {[...problems.values()].slice(0, 3).join(" ")}
                  {hasDuplicates(problems) && (
                    <button
                      className="link"
                      onClick={() => commit({ ...draft, fields: renameDuplicates(draft.fields) }, "dedupe")}
                    >
                      Rename duplicates
                    </button>
                  )}
                </div>
              )}
            </>
          ) : (
            <div className="se-json">
              <textarea
                className="mono"
                value={jsonText}
                onChange={(e) => setJsonText(e.target.value)}
                spellCheck={false}
              />
              <div className="se-toolbar">
                {jsonError && <span className="se-json-error">{jsonError}</span>}
                <span className="spacer" />
                <button className="btn small" onClick={() => navigator.clipboard.writeText(jsonText)}>
                  <Copy size={14} /> Copy
                </button>
                <button className="btn small primary" onClick={applyJson}>
                  <Check size={14} /> Apply JSON
                </button>
              </div>
            </div>
          )}
        </section>

        <section className="se-preview">
          <div className="subhead">
            <span className="pane-title">Preview</span>
            <span className="spacer" />
            <button className="link" onClick={() => setPreviewRow((r) => Math.max(0, r - 1))} disabled={previewRow <= 0}>
              <ChevronLeft size={14} /> Prev
            </button>
            <input
              className="cell mono num se-row-input"
              type="number"
              min={0}
              max={Math.max(0, schema.count - 1)}
              value={previewRow}
              onChange={(e) => setPreviewRow(Math.max(0, Math.min(schema.count - 1, Math.floor(Number(e.target.value)) || 0)))}
              title="Record index"
            />
            <button
              className="link"
              onClick={() => setPreviewRow((r) => Math.min(schema.count - 1, r + 1))}
              disabled={previewRow >= schema.count - 1}
            >
              Next <ChevronRight size={14} />
            </button>
            {preview && (
              <button className="link" onClick={() => setPreviewExpanded(new Set(allPaths(preview.nodes)))}>
                <ChevronsUpDown size={13} /> Expand
              </button>
            )}
          </div>
          {schema.count === 0 ? (
            <div className="empty-note">This list has no records to preview.</div>
          ) : !preview ? (
            <div className="empty-note">{problems.size ? "Fix the problems to see a preview." : "Decoding…"}</div>
          ) : (
            <>
              <FieldTree
                nodes={preview.nodes}
                expanded={previewExpanded}
                selected={previewSelected}
                onToggle={(p) =>
                  setPreviewExpanded((prev) => {
                    const next = new Set(prev);
                    if (next.has(p)) next.delete(p);
                    else next.add(p);
                    return next;
                  })
                }
                onSelect={(p) => {
                  setPreviewSelected(p);
                  const node = nodeAt(preview.nodes, p);
                  setPreviewRead(node && isUndefinedNode(node) ? node.off : null);
                }}
                onHover={setPreviewHover}
                onFollow={() => {}}
              />
              <div className="readings-slot">
                {previewSpan && previewRead !== null && (
                  <ReadingsCard
                    bytes={preview.bytes}
                    offset={previewRead}
                    span={previewSpan}
                    actionLabel="Add to schema"
                    onOffset={setPreviewRead}
                    onDefine={defineFromPreview}
                  />
                )}
              </div>
              <div className="subhead">
                <span className="pane-title">Bytes</span>
                <span className="spacer" />
                {highlight && (
                  <span className="muted mono">
                    {hex(highlight.off)}–{hex(highlight.off + Math.max(1, highlight.size) - 1)} · {highlight.size} B
                  </span>
                )}
              </div>
              <HexView
                bytes={preview.bytes}
                fileOffset={preview.fileOffset}
                highlight={highlight}
                focusKey={selected !== null ? `${selected}:${selectedRow?.off}` : null}
                onPick={(off) => setPreviewRead(undefinedSpan(preview.nodes, off) ? off : null)}
              />
            </>
          )}
        </section>
      </div>


      <footer className="se-foot">
        <span className="muted truncate" title={context?.userDir}>
          Each list you save is stored as {schema.targetLayout}list_{list}.json in {context?.userDir ?? "…"}
        </span>
        {context && (
          <button className="link" onClick={() => revealItemInDir(context.userDir).catch((e) => setError(String(e)))}>
            <FolderOpen size={13} /> Open folder
          </button>
        )}
        {context?.errors.length ? (
          <span className="se-json-error" title={context.errors.join("\n")}>
            {context.errors.length} layout file{context.errors.length > 1 ? "s" : ""} could not be loaded
          </span>
        ) : null}
        <span className="spacer" />
        <span className="muted">Ctrl+S save · Ctrl+Z undo · Esc close</span>
      </footer>
    </div>
  );
}

/** Editor for a field's conditional types: "if <field> is one of <values>, read as <type>". */
function RulesPanel({
  field,
  depth,
  controls,
  onChange,
}: {
  field: EditField;
  depth: number;
  controls: EditField[];
  onChange: (when: EditRule[]) => void;
}) {
  const kinds = ruleKindsFor(field);
  const names = controls.map((c) => c.name.trim());
  const update = (i: number, patch: Partial<EditRule>) => onChange(field.when.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const add = () => {
    const control = names.find((n) => n.toLowerCase() === "type") ?? names[0] ?? "";
    const other = kinds.find((k) => k !== field.kind) ?? field.kind;
    onChange([...field.when, { uid: uid(), field: control, values: [], not: false, kind: other }]);
  };
  return (
    <div className="se-rules" style={{ paddingLeft: 38 + depth * 16 }}>
      {field.when.length === 0 && (
        <div className="se-rule muted">
          Read this field as another type when a number field next to it has certain values, e.g. a float when type is
          7, 8 or 15–27.
        </div>
      )}
      {field.when.map((r, i) => (
        <div className="se-rule" key={r.uid}>
          <span className="se-rule-word">{i === 0 ? "If" : "Else if"}</span>
          <select className="cell" value={r.field} onChange={(e) => update(i, { field: e.target.value })}>
            {!names.includes(r.field) && <option value={r.field}>{r.field || "—"} (missing)</option>}
            {names.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
          <select className="cell" value={r.not ? "not" : "in"} onChange={(e) => update(i, { not: e.target.value === "not" })}>
            <option value="in">is one of</option>
            <option value="not">is not one of</option>
          </select>
          <ValuesInput value={r.values} onChange={(values) => update(i, { values })} />
          <span className="se-rule-word">read as</span>
          <select className="cell" value={r.kind} onChange={(e) => update(i, { kind: e.target.value as Kind })}>
            {kinds.map((k) => (
              <option key={k} value={k}>
                {kindInfo(k).label}
              </option>
            ))}
          </select>
          <span className="se-actions">
            <button
              title="Check this rule earlier"
              onClick={() => {
                const next = [...field.when];
                [next[i - 1], next[i]] = [next[i], next[i - 1]];
                onChange(next);
              }}
              disabled={i === 0}
            >
              <ArrowUp size={14} />
            </button>
            <button title="Remove this rule" className="danger" onClick={() => onChange(field.when.filter((_, j) => j !== i))}>
              <Trash2 size={14} />
            </button>
          </span>
        </div>
      ))}
      <div className="se-rule se-rule-foot">
        <button className="link" onClick={add} disabled={!names.length} title={names.length ? undefined : "No number field next to this one"}>
          <Plus size={13} /> Add rule
        </button>
        <span className="muted">
          Otherwise: <b>{kindInfo(field.kind).label}</b>
          {names.length === 0 && " · add a number field (such as type) next to this one to test it"}
        </span>
      </div>
    </div>
  );
}

function ValuesInput({ value, onChange }: { value: number[]; onChange: (values: number[]) => void }) {
  const [text, setText] = useState(formatValues(value));
  const [invalid, setInvalid] = useState(false);
  useEffect(() => setText((t) => (parseValues(t)?.join() === value.join() ? t : formatValues(value))), [value]);
  return (
    <input
      className={"cell mono se-rule-values" + (invalid || !value.length ? " invalid" : "")}
      value={text}
      placeholder="7, 8, 15-27"
      title="Values, separated by commas; ranges like 15-27 are allowed"
      onChange={(e) => {
        setText(e.target.value);
        const values = parseValues(e.target.value);
        setInvalid(values === null);
        if (values) onChange(values);
      }}
      spellCheck={false}
    />
  );
}

function DimsInput({ value, onChange }: { value: number[]; onChange: (dims: number[]) => void }) {
  const [text, setText] = useState(formatDims(value));
  const [invalid, setInvalid] = useState(false);
  useEffect(() => setText((t) => (parseDims(t)?.join() === value.join() ? t : formatDims(value))), [value]);
  return (
    <input
      className={"cell mono" + (invalid ? " invalid" : "")}
      value={text}
      placeholder="1"
      title="Array length, e.g. 5 or 2×3"
      onChange={(e) => {
        setText(e.target.value);
        const dims = parseDims(e.target.value);
        setInvalid(dims === null);
        if (dims) onChange(dims);
      }}
      spellCheck={false}
    />
  );
}

