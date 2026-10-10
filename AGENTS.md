# JD IDE — handoff notes for coding agents

Read this first, then `README.md`. The README is the user-facing manual and is kept in sync
with every feature; this file is what you need to work on the code.

## What this is

**JD IDE** is a desktop editor for Jade Dynasty (Zhu Xian, a Perfect World game) data files,
built with **Tauri 2** (Rust backend in `src-tauri/`, React 19 + TypeScript UI in `src/`).
Today it handles `elements.data` (all game items, NPCs, monsters, configs) across file
versions v66–v176: browse, search, check, compare, export, edit, and save. A separate
activity-bar workspace edits the client's `path.data`. The `tasks.data` workspace supports
verified v165, v172 and v184 task sets, including browsing, schema analysis, safe editing,
hierarchy operations and saving. More data files can follow later (`gshop.data`, …).

A previous Laravel/PHP version of the same idea lives in `C:/Users/mitko/Herd/jdide`. It is
a reference for ideas only; JD IDE replaces it.

## How the user works (follow these)

- **The user runs the app** (`npm run tauri dev`). Do not launch it yourself.
- **The user commits.** Never commit or push unless asked.
- **Saving `elements.data` was built last on purpose**; it is now done and confirmed working
  on a real server.
- They test in the real game/server and report back with concrete IDs and lists
  (e.g. "list 58, item 69329"). Reproduce against the real sample files before guessing.
- They like discussing larger design choices first. Some were **rejected** — don't
  reintroduce them:
  - free-form reorganising of fields in the schema editor (too confusing for new users);
  - an "add blank record" operation (create records by cloning or importing existing records).
- Plain, readable UI; small polish requests are common (alignment, visibility, wording).

## Commands

```bash
npm install
npx tsc --noEmit            # type-check the UI
npx vite build              # build the UI (also `npm run build`)
cd src-tauri && cargo test --lib   # Rust tests (183 at last run, ~6–7 min with the real task fixtures)
```

Environment quirks (Windows 11, Git Bash):

- `cargo` is not on PATH in non-login shells: `export PATH="$PATH:$USERPROFILE/.cargo/bin"`.
- Python is not installed; use `node` for scripts.
- **Do not run Prettier with defaults.** There is no Prettier config and the code is
  hand-formatted with long lines (~180 columns). Default Prettier rewraps whole files at 80
  columns. If you must format a new file, use `--print-width 180` and only on that file.
- Rust tests read real game files from `E:/` (override with `JDIDE_SAMPLES`) and skip
  samples they cannot find.

## Repository map

### Rust (`src-tauri/src/`)

| File | Role |
|---|---|
| `lib.rs` | Tauri commands and `AppState` (open document, compared document, catalog, settings, client resources). Every UI call lands here. |
| `settings.rs` | `settings.json` in the app config dir (client folder, open on start). |
| `elements/reader.rs` | Structural reader: header, lists, marker segments, talk block; `insert_record`/`remove_record` keep offsets in sync. |
| `elements/format.rs` | Layout catalog: built-in layouts (embedded `formats/`) + user overlays; `Ty`, `Field`, `ListDef`, markers. |
| `elements/mod.rs` | `Document`: opening (layout scoring, marker tables, detection), list resolution, records, summaries, find, talks, game text annotations. |
| `elements/decode.rs` | Record → field tree (`Node`), display roles, hints. |
| `elements/align.rs` | Borrowing definitions from other versions by aligning record sizes. |
| `elements/search.rs` | Advanced search: queries, conditions, slots, value parsing. |
| `elements/refs.rs` | References between lists, ID spaces (`registry_space`), "referenced by". |
| `elements/problems.rs` | Problems scan (duplicate IDs, broken refs, missing paths, …). |
| `elements/compare.rs` | Compare two files (summary, per-list diff, Markdown report). |
| `elements/export.rs` | Versioned JSON export of an item, a list, or search results. |
| `elements/import.rs` | JSON preview, list/ID matching, version/layout checks, row validation and stale-preview guard. Versioned JSON adds missing records from complete source bytes; additions and updates are one undo step. |
| `elements/coverage.rs` | Layout coverage report. |
| `elements/talk.rs` | NPC dialog (TALK_PROC) parsing. |
| `elements/edit.rs` | In-memory editing: journal, undo/redo, history, revert, clone, delete, bulk edit, value encoding. |
| `elements/save.rs` | Saving: checksum, backup, atomic write, changed-on-disk guard. |
| `client/mod.rs` | Game client folder: `path.data`, icon atlas, packages, string tables (`Resources`). |
| `client/pck.rs`, `client/dds.rs` | `.pck` 2.2/2.3 package reader, DDS decoding for icons. |
| `client/strings.rs` | `configs.pck` string tables and item name colours. |
| `client/titles.rs` | `interfaces.pck` `title_def_u.lua` title names/descriptions (parsed, never executed). |
| `path_data.rs` | Strict `path.data` PMID/GBK reader, validator and atomic writer. |
| `npcgen.rs` | `npcgen.data` (one server map's spawns): versions 1–14 reader/writer, document with journal (whole-item Replace/Insert/Remove per section), save. |
| `task_npc.rs` | `task_npc.data` reader/writer (12-byte header, 16-byte NPC_INFO records), save with backup and changed-on-disk guard. |
| `client/game.rs` | Character position from a running `elementclient.exe` (read-only): exe code scan for the pointer chain, process list, ReadProcessMemory. |
| `client/nearby.rs` | Entities a running client has loaded (NPC and matter hash tables): layout search, phase detection, collect. |
| `client/instances.rs` | Maps from configs.pck `Configs/instance.txt` (UTF-16 `"Name" { id zone "path" "data path" "detail" rows, cols … }` blocks, `//` comments): name, path, data path, rows, cols. |
| `dyn_tasks/format.rs`, `dyn_tasks/mod.rs` | `dyn_tasks.data` reader/writer (client limits, read-back check) and the open pack: edit journal, clone, delete, problems, save. |
| `tasks/container.rs` | Strict `tasks.data` index and numbered-pack reader: offsets, pack limits and MD5 validation. |
| `tasks/schema.rs`, `structures.rs`, `v165.rs`, `v172.rs`, `v184.rs` | Byte-preserving task schema engine and verified version layouts. |
| `tasks/browser.rs` | Lazy tree loading, background search/reference index, field inspection and hierarchy summaries. |
| `tasks/edit.rs` | In-memory task value edits, history, undo/redo, clone, delete and move/reparent operations. |
| `tasks/ids.rs` | Changing a quest's ID: index-based reference lookup, root rewriting, hierarchy links, one journal entry. |
| `tasks/dialogs.rs` | NPC talks as trees (`Dialog`/`DialogWindow`/`DialogOption`): read, validate, rewrite in the official editor's order. |
| `tasks/search.rs` | Advanced task search: field catalog, conditions (same-row groups, `any:` fields), value anywhere, scopes, parallel byte scan with prefilter, cancel and progress. |
| `tasks/save.rs` | Task-set staging, pack rebuilding, MD5/index updates, verification, backups and atomic replacement. |
| `tasks/analyze.rs`, `layout.rs` | Unsupported-version analysis, the built-in layout probe, and validated user task-layout patches. |
| `tasks/align.rs` | Proposes a user layout patch by aligning an unsupported task set with a supported one. |

### UI (`src/`)

- `App.tsx` — app shell: menus (File/Edit/Tools), panels, tabs, shortcuts, edit/save flow.
- `elements/api.ts` — one function per Tauri command. `elements/types.ts` mirrors the Rust
  structs (serde `camelCase`). **When you add a Rust command, add it to
  `generate_handler!` in `lib.rs`, to `api.ts`, and its types to `types.ts`.**
- `components/` — one component per panel/dialog (`AdvancedSearch`, `ProblemsPanel`,
  `ComparePanel`, `HistoryPanel`, `SaveDialog`, `SchemaEditor`, `RecordInspector`,
  `FieldTree`, `InlineEditor`, `PathDataEditor`, `TasksEditor`, `DynTasksEditor`, task hierarchy dialogs, …).
  `TaskDialogEditor` exports `DialogTreeEditor`, the talk tree editor both task editors use.
  `TaskNpcEditor` follows `PathDataEditor` (rows and undo history in the component).
  `NpcGenEditor` keeps the map in the backend (files up to ~1.4 MB) and fetches one item at a time; it
  reuses `NumberInput`, `TextInput`, `Field` and `VertInput` exported from `DynTasksEditor`.
- `schema/model.ts` — schema editor draft model; `schema/fieldList.ts` — pasted
  sELedit/Jade Editor field lists → fields.
- `elements/money.ts`, `time.ts`, `text.ts`, `talk.ts` — display helpers.
- `tabs.ts` — tab reducer (tabs follow records when rows shift).
- `App.css` — all styles, sectioned by feature. Colours are CSS variables (`--accent`,
  `--warn`, `--danger`, `--bg-*`, `--text-*`), light and dark.

### Data and tools

- `src-tauri/formats/layouts/<id>/` — built-in layouts (`layout.json` + `list_N.json`):
  v66, v112, v156, v156-signin, v158, v160, v165, v176. Embedded at build time.
- `src-tauri/formats/enums/`, `masks/` — built-in named sets. `build.rs` emits
  `rerun-if-changed=formats`: `include_dir!` does not track files, so new set or layout files were
  missing from `tauri dev` builds until a `.rs` file changed.
- User overlays (schema editor, enums/masks editor): `%APPDATA%\com.jdide.app\layouts\…`,
  `enums\`, `masks\`. They override one list or one set; everything else stays built-in.
- `tools/*.mjs` — layout generation/import pipeline (see README "Regenerating layouts").
  `tools/type-rules.json` holds `rules` (conditional types), `sets` (enum/mask by field
  name), `roles` (display roles) and `refs` (reference fixes); apply with
  `node tools/apply-type-rules.mjs`.

## elements.data — what we know (verified against the C++ source)

Reference source on disk:

- Client and editor: `E:/Game Dev/ZX-Client-Src/ZElement/` (`ZCommon/elementdataman.cpp`,
  `ZElementData/`, `ElementLocalize/DataManager.cpp`, `ZElementClient/EC_StringTab.cpp`).
- Server (v156): `E:/Game Dev/JD/zx_source/zgame/gs/` (`template/elementdataman.cpp`,
  `template_loader.cpp`, `gmatrix.cpp`). The sources are GBK-encoded.

Layout (`elementdataman::save_data`):

```text
u32 version (low 16 bits = format version, high bits 0x1000)
u32 export time (unix seconds)
lists     u32 item_size, u32 count, count × item_size bytes
markers   at fixed list slots, per version:
          checksum 8 bytes · exporter (0x19e75edf, len, XOR'd computer name, time) · tag (0xee35679f, len, data)
talk      u32 count, then TALK_PROC records (variable length) up to EOF
```

- Client and server files share this structure (both contain exporter/tag blocks).
- **Checksum**: the client's `load_data` (Windows builds) refuses to start on a mismatch:
  `MD5("ZPWDATA" + path.data + elements.data minus its first four 8-byte checksum slots)`,
  as 32 lowercase hex characters, 8 per slot. **Exactly four slots are skipped.** v165
  layouts mark a fifth 8-byte "slot" before list 296 (`bc 05 00 00 00 00 00 00`, which looks
  like an empty list header). It is hashed like normal data. Hashing it out was a real bug,
  now fixed and covered by `v165_hashes_the_fifth_slot`.
- Many private-server files carry stale checksums (edited by tools that don't update it).
  Only v156 (zxserver) and v165 (1792 server) validate with their own `path.data`. JD IDE
  always writes a correct checksum when it has a path.data.
- **ID spaces** (client/server file-loader `setup_hash_map`): Essence (items, types, services,
  plus WAR_ROLE_CONFIG and ITEM_TRADE_CONFIG), Addon, Recipe
  (RECIPE_ESSENCE/MAJOR/SUB types), Config (other `*_CONFIG`). Some later config
  `add_structure` overloads disagree with the loader; model the loader. One map per space; the later-loaded record wins. Duplicate IDs
  in one list are errors; across lists of one space they are warnings (shadowed).
- **Talk options** with the high bit set are functions from `SERVICE_TYPE`.
- **Texts**: names are `wchar_t[n]` UTF-16LE; some fields are GBK `char[n]`. The game stores
  line breaks as CR LF; the official editor's text exports write `!$`.
- Fixed-size text can fill all `n` characters/bytes without a terminator. Official data does this
  (for example NPC 54906's 16-character title); editing permits it and Problems keeps warning about it.
- **bool** in v112-era data is 4 bytes (an unsigned int); the v112 enums/masks were removed
  as outdated.

## Client resources (`client/`)

- Settings take the client root or its `element/` folder; `path.data` maps path IDs to
  resource paths; icons come from `surfaces.pck` (`iconset/iconlist_ivtr.dds` + `.txt`).
- `configs.pck` string tables (UTF-16LE, `#_index` / `#_begin`, literal `\r` = line
  break): `item_ext_desc.txt` (item descriptions), `monster_desc.txt`, `addon_str.txt`,
  `skillstr.txt` (name at skill id × 10, introduction at +1, detailed description at +2),
  `buff_str.txt` (name then description), `item_desc.txt` (palette entries 5–14),
  `item_color.txt` (name colours).
- `interfaces.pck` supplies `Interfaces/script/config/title_def_u.lua`. Its
  `title_definition` records map `id` to `note` (coloured title name) and `desc`; the editor
  parses those assignments without executing Lua.
- Skill and buff hover popovers keep and safely render `^RRGGBB` colour runs against a fixed
  dark background, so their game colours do not depend on the app theme.
- **Parsing rule (important):** a string starts on the same line as its number, but quoted
  strings can span physical lines (notably `skillstr.txt`). If another numbered quoted entry
  starts before the closing quote, treat the previous row as broken. Forsaken's
  `item_ext_desc.txt` has three such broken rows; blindly reading to the next quote used to
  drop 21,000 of its 26,057 entries.

## Editing model (`elements/edit.rs`)

- Edits change `Document.file.data` in memory immediately; saving writes those bytes out.
- Operations: `Set` (bytes in a record), `Insert` (clone), `Remove` (delete). Each keeps
  what it replaced, so undo/redo replays exact bytes.
- Records have stable **uids** (rows shift on insert/remove). `originals` keeps the first
  bytes of every touched record; `born` marks clones. Changed / added / deleted markers come
  from comparing against `originals`.
- History: every edit has id, label, time and field diffs. **Reverting one entry from the
  history does not add a new entry.** The entry shows "reverted" (with the time), and
  reverting an already reverted entry does nothing. This fixed an infinite back-and-forth
  revert loop the user reported.
- **Clone ID**: the list's highest ID + 1, moved past any ID used in the same ID space
  (`next_free_id`). The user first asked for "max + 1" and then agreed to respect the ID
  space, as the client does.
- **Delete** asks first and lists the records that refer to the deleted one ("Referenced by").
- **Bulk edit** runs from search results, on all hits or only the picked ones.
- **Compare copy** transfers selected compatible fields from the compared document into existing records of the open document. Missing records can be selected in bulk only when the elements version and structural list fingerprint match; their IDs and complete bytes are preserved. One selection is one undo/history entry.
- `Journal::mark_saved` (on save): edits count from the saved file from then on (markers
  clear, Revert all goes back to it), while undo still goes back past it. `EditState.lastSaved`
  and `HistoryEntry.savedAt` drive the "Saved" line in the history.

## Saving (`elements/save.rs`, `SaveDialog.tsx`, `UnsavedDialog.tsx`)

- Writes the data with the export time set to now and a new checksum, using the path.data
  found next to the target, else next to the open file, else the client folder's, else one
  chosen in the dialog.
- Writes to `<name>.jdide-saving`, then renames it over the target; clears the read-only
  flag (as the official tools do).
- Backup: on the first save over a file in a session, the old file is copied to
  `elements.data.YYYYMMDD-HHMMSS.bak` (a checkbox, remembered in localStorage).
- Changed-on-disk guard: a quick Ctrl+S save fails with a `CHANGED_ON_DISK` prefix when
  another program changed the file since it was read; the dialog then shows the warning
  and sends `replaceChanged: true`.
- The first save opens the dialog; later Ctrl+S saves straight away with the same choices.
- Closing the window or opening another file with unsaved edits asks Save / Don't save /
  Cancel. This needs the `core:window:allow-destroy` capability.
- Verified on real data: an unchanged save differs only in time and checksum; a server
  loaded a saved file. (A crash during testing turned out to be duplicate map tags in the
  user's server config, not the file.)

## Display roles, sets and the schema editor

- Roles (`display` on a field): `path`, `icon`, `skill`, `buff`, `title`, `time` (unix),
  `duration` (s), `duration_ms`, `daytime` (seconds into the day), `money` (copper; shown
  as `12G 34S 56C`, full words on hover; editing accepts `1G 50S`, `1g 50s` or
  `1 Gold 50 Silver`). The user wanted capital letters or full words, never `g/s/c`.
- Enums and masks are named sets; one set per meaning (Chinese duplicates were merged).
  Sets can be created, edited, deleted, or reverted to built-in. The 64-bit mask popover
  sizes its hex column with `--hex-w`.
- Schema editor: types, arrays, structs, groups (group/ungroup, the group checkbox selects
  its fields), optional colours for group/struct headings, conditional types, refs, roles,
  Import from other versions, and
  **Paste a field list**:
  - sELedit/Jade Editor format: a line of `;`-separated names, then a line of types;
  - types: `int32`, `float`, `wstring:N`, `byte:N`, `byte:AUTO`, …;
  - **N is in bytes, so `wstring:64` = 32 chars**. Confirmed by v156 EQUIPMENT_ADDON being
    88 bytes; the Jade Editor configs are in `E:/Game Dev/JD/Tools/Jade Editor/assets/elem_cfg/`.
- Schema edits never change data bytes, only how they are read.

## tasks.data editor

- Supported task formats are v165, v172 and v184. The index, every numbered pack, root offset
  tables and stored pack MD5 values are checked before tasks are shown.
- The browser loads roots lazily, builds nested-task search and reference data in the background,
  and exposes a searchable read-only schema browser for supported versions.
- Unsupported versions open in the task layout analyzer. It compares against a supported baseline,
  scores fixed-width insertions, supports guarded schema patches, and permits promotion only after
  every root decodes and re-encodes byte-for-byte.
- Ordinary edits cover fixed-width values and variable task text while shape-changing controller
  fields remain locked. All changes are journaled with undo/redo and history.
- **Clone task** copies a complete top-level tree with fresh IDs for its root and descendants and
  remaps internal task references. It appends to the source pack when it has capacity, otherwise to
  another existing pack below the 300-root limit. Creating a new numbered pack is not implemented.
- **Clone subtree**, **Delete subtree** and **Move subtree** operate below an existing root. Delete
  previews references that would become unresolved; move supports destinations in other roots and
  packs while preserving IDs.
- **Top-level deletion** (`delete_subtask` with an empty path → `delete_root`): the root becomes a
  tombstone, `modified[(pack, root)]` with an empty `current`. `current_root_optional` returns none for it,
  its summary and index entries are removed (`forget_root_in_index` keeps it among `edited_roots` so a late
  background index cannot bring it back), and undo/revert restore it in place (`insert_root_summary` keeps
  (pack, root) order). Positions never shift during a session. On save, `stage` skips tombstones,
  `closing_positions`/`renumber_roots` move later roots up in the summary and index, and the history is
  cleared (`has_deleted_roots`). Unsaved appended clones are removed directly (last one only). The client
  loads empty packs (TaskTemplMan::LoadTasksFromPack loops over `item_count`). The delete preview's
  `lost_ids` drive the elements.data places (`Document::clear_task_id_uses`, set to 0; DlgNPC and
  EC_NPCServer skip 0 in `id_tasks`). Covered by `deleted_top_level_task_leaves_the_pack_on_save`.
- Saving a top-level clone rebuilds its destination pack's offset table and MD5 and increments the
  index root count. A save that adds or deletes top-level quests changes root positions, so it starts a
  new task undo history.
- `cloned_top_level_task_rebuilds_the_pack_table_and_index_count` covers the structural save/reopen
  path. The full Rust library suite, TypeScript check and Vite build passed after this feature.
- On October 8, 2026 the user cloned and saved a real v165 top-level task, then confirmed that both
  the matching server and client started successfully without a crash.
- **Hierarchy links** (verified against `TaskTempl.h` and all three client fixtures, 100% of tasks):
  the last 16 bytes of every task's fixed block hold its parent, previous sibling, next sibling and
  first child IDs (top-level tasks: zeros), refreshed by `ATaskTempl::SynchID` before official saves.
  The game recomputes them after loading. Structural operations call `sync_hierarchy_links`, which
  rewrites them for the whole root only when the source root was already consistent.
- Fresh clone IDs come from `fresh_task_ids`: above the index maximum and `id_floor`, the highest
  ID cloned or deleted this session, so freed IDs are never reused.
- Undo/redo apply the journal's changes first and only then move the entry (`commit_undo`/
  `commit_redo`), so a failed apply leaves history and data in step.
- `move_task_subtree` takes the expected source and destination task IDs (`check_task_id`) so a
  move prepared before an undo cannot act on other quests.
- UI (`TasksEditor.tsx`): every operation runs through `beginOperation`/`endOperation`; list
  clicks (`userSelect`) and undo/redo wait while one runs or a task dialog is open. After clone,
  move, delete, undo, redo and revert all, `resync` reloads the root list (`task_summary`), drops
  trees and nested expansion of changed roots, and reselects the focused quest by ID.
- Task history (`TaskHistoryPanel.tsx`) replaces the list and inspector like the elements history.
  `revert_entry` applies the inverse of an entry only while every root it changed still holds its
  result (root-level snapshots cannot merge); otherwise `revert_blocked` names the later edit. A
  revert is recorded with `reverts: Some(id)`, is not listed, and marks the original as reverted;
  reverting it again does nothing. `Journal::mark_saved` drives the Saved line.
- **Premise/mutex fields** (`v165::premise_fields`) split the former `unknown_0806_1317` block using
  `TaskTempl.h` offsets anchored on `premise_title_count` (802) and `teamwork` (1318); v172/v184 only
  grow `premise_friendship` from 32 to 48 (`set_friendship_count`). `schema::is_task_reference`
  is the one list of task-ID field names (index, inspector links, edit validation, clone remapping);
  `is_element_reference` lists item/monster/object fields. The link fields are `LINK_FIELDS`.
- The background index (byte probe in `schema.rs` and `collect_search_entries` for edited roots)
  records task and element references with dotted paths (`fixed.premise_tasks[0]`) and each task's
  stored links. `tasks/problems.rs` scans only that index; `task_problems` collects element IDs,
  checks them against the open elements.data, then scans, never holding both locks at once.
  `referenced_by` serves the inspector's Referenced by section.
- **Task JSON** (`tasks/json.rs`, `TaskImportDialog.tsx`): `export_tasks_json` writes
  `jdide-tasks` v1 with `taskVersion` and `schemaDigest` (MD5 of the serialized schema; user
  layouts can differ under one version). `import_tasks_json` previews without a token and applies
  with one (MD5 of input, path, `Journal::generation` and digest). Field values go through
  `browser::set_task_field`, the same validation as inspector edits (`edit_field` calls it too).
  Additions append decoded `_raw` roots as `RootChange`s with empty `before`, after the edited
  roots, in pack/root order.
- **Change quest ID** (`tasks/ids.rs`, `TaskIdDialog.tsx`, commands `preview_task_id_change`, `change_task_id`):
  references come from the background index (non-element references to the old ID, dialog option
  parameters included); each affected root is rewritten with `rewrite_internal_task_references` (the
  clone remapping), the quest's own root gets `fixed.id` and `sync_hierarchy_links`; all roots are one
  journal entry; `id_floor` moves past the old ID. With duplicate old IDs only the quest's own root is
  rewritten. The ordinary `fixed.id` edit stays locked. elements.data: `Document::task_id_uses` scans
  4-byte slots whose paths pass `elements::edit::is_task_id_path`: a quest-ID name (`is_task_id_field`,
  checked against ForsakenJD: exactly the C++ task fields, e.g. `NPC_TASK_OUT_SERVICE.id_tasks`,
  `MINE_ESSENCE.task_in`, `PIE_LOVE_CONFIG`; not `id_task_set`, `Task_Start_Map`) or an `id` directly inside a
  quest-named group (`TASKDICE_ESSENCE.task_lists[].id`, the only such field in any layout; user report); `replace_task_id_uses` writes the ticked places as one elements
  journal entry, refusing places that no longer hold the old ID. The commands lock tasks, release, then
  lock the document (never both). TasksEditor passes the elements EditState to App's `afterEdits`.
- **Dialog editor** (`tasks/dialogs.rs`, `TaskDialogEditor.tsx`, commands `task_dialogs`, `set_task_dialog`): the
  UI sends a whole talk (windows with texts and options; option `target` = window ID or `0x80000000 |
  function`), `set_dialog` checks it is a tree (every window opened by exactly one option, no missing
  targets, texts ≤ 63 units, quest parameters exist) and writes it depth-first from the root like
  `CTalkModifyDlg::FillWindowData` (ZElementData/TalkModifyDlg.cpp), setting `parent_id` (-1 for the root)
  and counts. Unchanged windows and options keep their nodes, so a no-op write is byte-identical; new ones
  come from `browser::empty_node`. Window text follows the task's terminator convention (NUL inside the
  text in v165). Client (DlgNPC.cpp): `windows[0]` opens first, windows are found by ID, Back uses
  `parent_id`; a talk shows only with >1 window or text in the first. Sample data: all 24,456 ForsakenJD
  talks are trees in that order (HDN: one unreachable window). Talks: delivery (`CanDeliverTask`),
  unqualified (requirements fail), execution (`GetUnfinishedTalk`), award (`CanFinishTask`), item_delivery.
- **Advanced task search** (`tasks/search.rs`, `TaskSearchPanel.tsx`, commands `task_search_fields`,
  `search_tasks_advanced`, `cancel_task_search`, `task_search_progress`): `SearchSource::of` snapshots the
  container, schema and unsaved roots under the lock; `run` scans packs on all cores without it, using
  `visit_task_leaves` (the index probe with a visitor; no `Node` trees). `field_catalog` lists leaf paths
  without indexes; structures listed in `ANY_ROOTS` (`AWARD_DATA`, `TASK_TALK`) give `any:award:…` /
  `any:dialog:…` fields covering every copy; a structure nested in itself (selected-role awards) is listed
  one level deep (otherwise 32,942 fields instead of 3,734). Same-row groups use the innermost list all
  fields share, in each condition's own suffix terms, and compare row prefixes (`LeafPath::prefix`). With
  "all", `= ID`/`is one of` on integers ≥ 256 skip roots lacking the value's low two bytes. Speed in a debug
  build: ~3.5 s for a full ForsakenJD scan, ~0.6 s for an ID. The probe keeps its path as `Segment`s (strings
  only on demand), takes `probe_needed_names` built once per scan (it was rebuilt per root) and hashes
  names with FNV (`NameSet`); in visitor mode it skips reference collection. These halved the walk.
  `describeTaskField` (`TaskForm.tsx`) names fields and picks the tab a result opens on.
- **Task compare** (`tasks/compare.rs`, `TaskComparePanel.tsx`): `ComparedTasks` opens and indexes
  the other set (`AppState.compared_tasks`; lock order tasks, then compared_tasks). `compare_with`
  pairs unique IDs, skips byte-identical roots at the same path, decodes the rest in parallel, and
  reports per-task field counts only; `task_compare_fields` returns one task's diffs. Copy turns the
  selection into import rows (`ComparedTasks::copy_rows`, `resolve_copy` expands whole tasks) and
  applies them with `TaskDocument::copy_rows` (JSON planner, additions only if `same_layout`).
  v165 vs v172 compare: ~31 s in a debug build, 27,629 changed pairs, 625 KB report.
- **Task translation** (`tasks/translate.rs`, `TaskTranslateDialog.tsx`): `TranslationSource`
  (`AppState.task_translation`; lock order tasks, then task_translation) holds the source
  `ComparedTasks` and the previewed `RootEdits` (task path, field, group, text per root). Preview
  decodes paired roots in parallel and trial-applies each text with `set_task_field`; apply only
  replays the stored edits of the chosen groups on the unchanged roots. v165 from v172: ~16 s debug,
  718 names, 473 descriptions, 11,687 dialog texts, 23 talks skipped for shape.
- **Fixed header names** come from parsing `ATaskTemplFixedData` (packed; pointers 4 bytes,
  `vector`/`abase::vector` 16, `ZONE_VERT` 12, `task_tm` 24): all anchors and the 2,490-byte
  total match. v172 `kermis` sits after `faction` (same-ID flag agreement with v165: 8 differences
  of 67,250); v184's `unknown_v184_1..3` sit after `premise_has_king` (only placement where every
  completion method is plausible). Pointer members stay `raw(4)` named `*_pointer`.
- The byte probe keeps only scope values that conditions and counts read
  (`probe_needed_names`); recording every header field slowed indexing by ~45%.
- **Quest form** (`TaskForm.tsx`): `LAYOUT` maps tabs to groups of field paths (grids, tables, id
  lists, the class checklist, friendship tables, dialogs, and "rest" predicates for unplaced header
  fields); every field of a group is shown (the user preferred this over folding unused fields).
  Rewards has its own view (`RewardsView`/`AwardEditor`, sub-tabs in `AWARD_TABS`). Single values go
  through `editField`; multi-value edits (class list + count) through `edit_task_fields`, one journal
  entry. The Advanced tab is the previous categorized field tree.
- Form tables and lists use `Cell` (input, applies on Enter/blur). Row operations go through
  `edit_task_array` (`TaskDocument::edit_array`, `ArrayEdit` add/clone/remove, one journal entry):
  `CountedArray`s change length and their (locked) count field; `FixedArray`s need `count_path`
  (a non-structural count, from the layout's `count`) and keep their length, used slots first. New
  rows are decoded from zero bytes with the list's item type. `given_items` row operations recount
  `given_common_item_count`/`given_task_item_count` (the client also recomputes them on load).
  `companions` are fixed lists whose slots pair with the rows (scaled awards: `awards` counted by
  `scale_count`, paired with `ratios[5]` or `counts[5]`): they get the same insert/copy/removal, keep their
  length, and cap the rows (5). Selection rules (server zgame/gs/task/TaskTempl.inl CalcAwardDataBy…):
  ratio = first with time ratio ≤ ratios[i]; items = first with item count ≥ counts[i]; count = from the
  last back, first with finish count ≥ counts[i]. `SCALE_RULES` in TaskForm.tsx shows them.
- **Talk option parameters**: for functions 0, 6, 7, 8 and 21 (`TASK_OPTION_FUNCTIONS`; NPC_TALK,
  GIVE_TASK, COMPLETE_TASK, GIVE_TASK_MATTER, GIVEUP_TASK) `parameter` is a task ID. Verified on
  ForsakenJD (function 6: 8,834 options, 6,878 to their own task; 17/18 and window links never use
  it). Clone remaps it (`rewrite_internal_task_references`), the index and probe record it as a task
  reference, and the inspector links it. Before this, a cloned quest's NPC kept giving the original.
- Task class IDs are `CHARACTER_CLASS_CONFIG.character_class_id` (verified: common class lists such
  as 25–29 and 96–100 are whole factions there); `Document::character_classes` serves them. Jade
  Editor's class list order does not equal these IDs, so its names are only used where the IDs are
  certain (`formats/enums/task_occupation.json`). Friendship names are by index (`task_friendship`).
- The class checklist is laid out by `CLASS_GROUPS` in `TaskForm.tsx` (race → class → pre-tier and
  tiers 1–5), from the user's ID list (e.g. Arden Tier 1 = 39; Deikin 逐霜 has a pre-tier, 249). It
  matches ForsakenJD and HDN elements.data. Tasks also use 0 (Novice), GM 15/18/21/24 and 32, 107,
  122, which no client defines; they appear under Other classes.
- Task value names (`TaskForm.tsx` `ENUM_FIELDS`/`MASK_FIELDS`, sets `formats/enums/task_*.json`,
  `formats/masks/task_recommend_type.json`): `task_type` from `DlgTask.h` `TaskType` (template value =
  TT − 1, 0–20) with English names from `interfaces.pck` strings 3101 + type; the English client
  says Clan (7) and Vitalic (14) where the source says 修真 and 跨服. `display_type` strings
  13410 + type − 1 (0 is derived on load, never stored by the official editor). `recommend_type` is a
  bit mask, bits 1–8 (`RECOMMEND_TYPE_*`, strings 13420–13426). `rank` is 1–5 stars. Value ranges
  in all three fixtures match these sets; `dynamic_task_type` is always 0.
- More value names (ENUM_FIELDS/MASK_FIELDS keys are a field name or "list.name", see `setFor`):
  summon_mode (TaskTempl.h comment), premise_fengshen_type, finish_time_type (GetFinishTimeLimit: day/week),
  life_again_count_compare and refine_condition (server checks), compare `operator` and joins
  (CheckGlobalExpression(s)), message channels (znet macros.h GP_CHAT_*, gsp_if.h speaker/rumor),
  special_award_type, `parameter_tokens.type` (TaskExpAnalyser.h), parameter_expression_selection,
  TASK_TIME weekday (task_week_map: 1 = Monday), timetable types (enumTaskTime*, one byte per row in
  `fixed.timetable_types`, edited through the form; fixed `Bytes` up to 64 bytes are editable as hex, kept
  as `Bytes` so task JSON schema digests do not change). Masks: premise_cult/cultivation → `god_devil_mask`,
  premise_nation_position_mask → `nation_position_mask`, selected_role → `task_selected_role`
  (DeliverAwardToSpecifyRole). clear_cultivation_skill bits are unknown (skill library not in the source).
- **Unsupported versions.** `probe_layouts` decodes ~150 spread roots with every built-in
  layout. User layouts are frozen: `Schema::frozen_at(base)` settles `since/until` version
  conditions, so a layout based on a newer version reads an older file the same way (v170 reads
  exactly with v172). `align::propose` pairs roots by ID (400 spread plus quests where rare fields
  are set), walks the reference leaves with a running byte shift (strong ≥2-byte non-zero values
  re-anchor, one-byte values only confirm, texts carry their length difference, counted lists with
  other counts are skipped) and records per-instance marks. The event solver (support ≥ 10) is now
  only the fallback (and the source for the task record itself, `schema.root`). `solve_all` solves
  structures with a DP (`solve_structure`/`solve_flat`): anchor = record start (roots), first field, or
  the earliest well-matched field whose value does not usually equal a neighbour's (three 1.0
  coefficients shifted results by 4 bytes); forward and backward passes keep/remove/shorten fields
  and insert bytes, scored by 2·matched − compared non-zero reference bytes (texts blanked) minus a
  cost per change; end marks (where the next instance starts, second round) reward the right total.
  Inner structures first; their changes become per-pair byte adjustments for the outer ones.
  Unsolved fixed-size nested structures are flattened (`flatten`); the majority answer per inner
  structure wins. Count/condition fields are never removed (`referenced_names`). Tie-breaks: inserts
  earliest; removals prefer `unknown_*`, `*_pointer(s)`, `*_capacity`, then later fields. Results:
  v172 from v165 exact (kermis after faction, friendship 32→48); v170 none; v186 Reborn from v184:
  header −387 (pointers gone, have_fail_items 16→4, life_again lists 45→1, …), awards −148 with an
  ambiguous 4-byte split, 406/1111 sample and ~30% of all roots exact; most failures stop at
  `success_award.extra_tribute` (v186 has 8 more bytes after the candidate list).
- XtremeJade v165 scan (official data): 15 duplicate IDs, 32 broken references (e.g. four "Join …"
  quests awarding missing task 2608), 1 self-reference, 0 stale links, 44 full packs.

## dyn_tasks.data editor

- Format from `ATaskTemplMan::UnmarshalDynTasks` / `ATaskTempl::UnmarshalDynTask`
  (ZElementClient/Task): 12-byte header (pack_size = file size, time_mark, version 13, task_count),
  then tasks: mask (bits 0–12 = optional sections), mask2, type, special award (top-level type 1),
  id, u8-length UTF-16 name, 17 flag bytes, level min/max, sections, method + goal data (1 kill,
  2 collect, 4/13 site, 5 wait), finish type, award (mask: gold, u64 exp, SP, reputation, item
  groups), 3 i32-length texts, 5 talks (prompt and option texts by byte size without NUL; window
  text by units with a NUL; window/parent IDs are signed bytes), i32 subtask count, subtasks.
  ITEM_WANTED 31, MONSTER_WANTED 22, task_tm 24 bytes. HDN/Reborn move the item groups from award bit
  4 to bit 5 (bit 4 there holds something of unknown size no pack uses); `format::read` tries both.
- The official writer's deposit section reads instead of writing (`MarshalDynTask` bug); JD IDE
  writes the value.
- Client: `VerifyDynTasksPack("userdata\\dyn_tasks.data")`; on a time-mark mismatch the server sends
  the pack and the client overwrites userdata. Dynamic tasks are mounted on NPC services 1403/1404
  (`UpdateDynDataNPCService`). IDs share the task list with tasks.data (4928–38960 in ForsakenJD).
- All packs are special-award gifts (诛仙的馈赠); special award numbers repeat in official data.
- `DynDocument` keeps each top-level task's bytes; `set_task` writes the edited task and requires
  `write_task_bytes` to read back equal (given-item counts are recounted). Journal entries hold
  whole-task Replace/Insert/Remove changes. Clone IDs: above this pack, the open tasks.data
  (`TaskDocument::task_names`, the background index) and `id_floor`.
- Lock order: `tasks`/`document` are read first and released, then `dyn_tasks`, then `compared_dyn`.
- Overview: `DynDocument::overview` (award summary per top-level task). Compare: `ComparedPack`
  (`AppState.compared_dyn`, read-only), `compare` pairs by top-level ID (first of duplicates) and names
  differing parts by comparing the tasks' serialized top-level keys; `copy_clash` blocks ID clashes
  (other tasks here, the open tasks.data) and item rewards into a pack of unknown layout; `copy_from`
  writes with this pack's layout, appends missing tasks and replaces existing ones in one journal entry.
- Item/monster picker: `Document::essence_spec` (monsters: `MONSTER_ESSENCE`; items: essence lists
  that are not types, services, configs, `MONSTER*`, `NPC*` or `MINE*`) + `picker_records`, command
  `pick_essence`; `ValuePicker` takes a `search` function instead of an elements field.

## task_npc.data editor

- `ATaskTemplMan::UnmarshalNPCInfo`: header u32 pack_size (= file size), i32 time_mark, u16 version 2,
  u16 count; records `NPC_INFO` {u32 id, i32 map_id, i16 x, y, z} declared before `#pragma pack(1)`
  in TaskTempl.h, so 16 bytes with 2 padding bytes (always 0; kept). Stored in a hash map by id (the
  last record wins); files are in hash order. Client: `LoadNPCInfoFromPack("data\\task_npc.data")`,
  used by DlgTask/DlgTaskBase (minimap target, fly links). Server: gs.conf `QuestNPCInfo`,
  `PlayerTaskInterface::GetTaskNPCPos` (teleport). No time-mark sync between them.
- Six fixtures (XtremeJade, 1559, ForsakenJD, HDN, Reborn, zxserver) round-trip; ~45% of records have map 0.
- Map names: `Resources::instances`; HDN's instance.txt has comments after values. ForsakenJD has
  295 maps (401 = New Main City). Picker kind `npc` = NPC_ESSENCE + MONSTER_ESSENCE.

## npcgen.data editor

- Format from `CNPCGenMan::Load` (ZElementEditor/NPCGenData.*, the same code as the server's
  zgame/gs/template/npcgendata.*): `#pragma pack(1)`, `size_t` = 4 bytes. u32 version; header counts
  (dynamic objects v6+, controllers v7+); areas (+ctrl/life/max v7+, export/attach v12+, phase v14;
  60-byte generators; attached export IDs when attach_num > 0, −1 = this area is attached); resource
  areas (dir/rad v6+, ctrl/max v7+, export/attach v12+, phase v14; 20-byte resources); dynamic objects
  (scale v9+, controller v10+, phase v14); controllers (128-byte GBK name; range v8+, repeat v11+, v13+
  i32 segment count in the low 16 bits with logic in the high bits, then 2 × 24-byte times per segment).
- Only the server loads it (gs.conf `NPCGenFile`, one per map folder; `instance_manager.cpp`). The
  client's `element/data/npcgen.data` is identical in all three clients and unused.
- 519 sample files (zxserver, 1559, ForsakenJD) round-trip; versions 4, 8, 9, 11, 12, 13, 14.
  `encode` refuses values a version cannot store. Controller names keep their raw bytes while unchanged.
- Picker kind `mine` = MINE_ESSENCE. The plot draws x and −z.
- Map image (`client_maps` command, `jdmap://localhost/<generation>-<path>` protocol,
  `Resources::midmap_png`, a few PNGs cached): `Surfaces/MidMaps/<path>.dds` (DXT3/5, 524 or 1036 px
  for 1 or 2 rows; the 12 extra pixels are part of the scaled image). `CDlgMidMap` maps the whole
  texture onto x and z from −rows × 512 to +rows × 512 (it uses rows for both axes), north at the top.
  Minimaps (the radar, `CDlgMiniMap`) use a padded `2 + 2·cols` layout and are not used.
- **From game** (`client/game.rs`, commands `game_clients`, `game_position`): position =
  `[[[g_pGame] + game_run] + host_player] + 0x3C` (A3DCoordinate: vtable, class ID, AString name, then
  `m_matAbsoluteTM`; row 3 = position). `g_pGame` and both member offsets differ per build, so
  `find_chain` scans the exe's code for `mov r,[abs]` → `mov r,[r+X]` → `mov r,[r+Y]` → a read of
  +0x3C/0x40/0x44 (mov, fld, movss) and takes the clear winner (≥ 4 votes, ≥ 3× the runner-up).
  Results: XtremeJade 0xCF5E24 +1C +2C, ForsakenJD 0xE18F9C +20 +2C, HDN 0xFBC704 +20 +30, Reborn
  0x1130DB4 +20 +30 (13/10 votes vs 1). Our source matches XtremeJade only (ForsakenJD's CECGame and
  HDN's CECGameRun have one extra member). All four exes have a fixed base (no ASLR); relocatable exes
  use the module base from a Toolhelp snapshot. Chains are cached per exe path, size and mtime.
  Facing: row 2 at +0x2C (`GetDir`, what the client sends to the server via `glb_CompressDirH`), read
  together with the position and normalized. Spawn areas store that vector (on the ground plane).
  Resource areas and objects store axis + angle (AIGenExportor: `quat.ConvertToAxisAngle`,
  `a3d_CompressDir(axis)`, `_RadianToChar(angle)` = angle / 2π × 255 truncated; the Rust field
  `radius` is this angle, `rad` in the C++). From Angelica3D_s.lib (dumpbin /disasm):
  `a3d_DecompressDir(b1, b2)` = (cos a·sin e, cos e, sin a·sin e) with a = b1, e = b2 in 1/256 turns, so
  upright = (0, 0); `a3d_CompressDir` = (atan2(z, x), acos(y)); quaternions are D3DX
  (`AxisAngleToQuad` standard, `QuadToMatrix` = D3DXMatrixRotationQuaternion). An upright turn θ
  faces row 2 = (sin θ, 0, cos θ), so turn = atan2(x, z). Unrotated items carry axis (192, 64/63) with
  turn 0 (the axis of an identity quaternion).
  `follow` rejects null pointers ("not in the world") and non-finite or huge values. Opens the process
  with PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ only (`windows-sys`, Windows only). No
  anti-cheat DLLs in the four client folders. Not yet checked against a live client.
- **Nearby fetch** (`client/nearby.rs`, `NpcGenNearby.tsx`, commands `game_nearby`, `import_npcgen_nearby`):
  `CECGameRun::m_pWorld` → `CECWorld::m_aManagers[6]` (player, NPC, matter, ornament, skill gfx, decal) →
  `abase::hashtab<Object*, int>` (`{hash, count, vector{data, finish, max, size}}`, nodes `{next, value,
  key}`). Keys: NPC IDs `0x8…` without `0x4…`, matter IDs `0xC…`. `CECNPC::INFO {nid, tid}`,
  `CECMatter::INFO {mid, tid, dropper_id, dir0, dir1, rad}`; dynamic objects are matters with tid
  `0x80000000 | id`. `find_layout` accepts six distinct manager pointers, a table whose walk equals its
  count, and an info offset where 12 objects store their own key; `find_phase` needs the only offset with
  `bool m_bPhase`/`short m_iPhaseId` semantics and ≥ 2 phased objects (unverified live: no phases in z1).
  Layouts are cached per exe for the session. Live ForsakenJD (z1, Oct 10 2026): world +0x08, managers
  +0x24, NPC table +0x10 / info +0x11C, matter table +0x10 / info +0x10C; search 35 ms, collect < 1 ms;
  collected NPC positions equal the zxserver/1559 z1 npcgen.data spawns. Classification (`lib.rs`):
  `Document::essence_struct` (MONSTER_ESSENCE → monster, other → npc, missing → unknown; matters:
  dropper or non-MINE_ESSENCE → item). `Document::import_nearby` (one journal entry, `record_all`):
  point spawns as official ones (kind 1, init/revive/valid once 1, NPC refresh 0), resource type 47
  (`DT_MINE_ESSENCE`; the server ignores it), export IDs after the section's highest (v12+), phase v14.
- Servers name map folders by the instance's **data path**, images use its **path** (`e12` vs `z12`
  for Foxhill). `detectMap` (NpcGenEditor) tries `npcgen_<map>.data`, then the folder, data path
  first; zxserver/1559: 99/139 map folders match (83/119 with an image); `b31`, `d12`, `t01–t03`
  and `empty` do not. The per-file choice is in localStorage (`jdide.npcgen.mapFor`).
- Server semantics (zgame/gs/npcgenerator.cpp `LoadGenData`, value names from ZElementEditor
  SceneAIGenerator.cpp/NpcPropertyDlg.cpp/AIGenExportor.cpp):
  - Extents are **full sizes**: `rect = pos ± ext × 0.5` (areas and resource areas). The editor writes
    EdgeLen/EdgeHei/EdgeWth. Area `kind` (iType): 0 follows the terrain, 1 plane or box (fixed height;
    older tools say "Fly").
  - `npc_type`: 1 server NPC, 2 interaction (mobactive), else monster; `group_type` (monsters): 0 mobs,
    1 group, 2 boss spawner. `revive` (cReviveType): 0 none, 1 after disappearing, 2 when switched on
    (the editor refuses 2 for NPCs). `phase` = SetPhaseID; older tools label it "Buff Region"
    (x1 export 415 holds 26).
  - Area/resource/object `controller` (idCtrl) refers to `Controller.id`; `controller_id` is the
    trigger ID (TranslateCtrlID).
  - Generators: aggressive 0 template / 1 aggressive / 2 passive; respawn = 15 (BASE_REBORN_TIME) +
    refresh, clamped 15 s–30 days; corpse delay 0 = default, else 5–1800; `loop_type` 0 stop at end,
    1 back along the path, 2 loop; `speed_flag` 0 walk, 1 run; faction overrides apply when the
    `default_*` flag is 0 (server bug: accept-help takes the flag, not the value). `died_times`
    (editor default 50), `offset_water` and `need_help` are not read by the server.

## Sample files (for tests and repros)

| Path | Version | Notes |
|---|---|---|
| `E:/Game Dev/JD/zxserver/zgame/gs/config/elements.data` (+ `path.data`) | v156 | Matches the source; valid checksum. Default test file. |
| `E:/Game Dev/JD/1559/gamed/config/elements.data` | v158 | server |
| `E:/Game Dev/JD/Clean/root/gamed/config/elements.data` | v160 | server |
| `E:/Games/ForsakenJD/element/` (client: `data/elements.data`, `configs.pck`, `interfaces.pck`, `surfaces.pck`) | v160 | The user's main client; has a `.bak` from a save. |
| `E:/Game Dev/JD/1792/gamed/config/elements.data` | v165 | server; valid checksum with its path.data |
| `E:/Games/Elite Jade Dynasty - HDN/element/` | v165 | client |
| `C:/Users/mitko/Desktop/elements-v112.data`, `elements-v156.data` | v112/v156 | loose copies |

Task fixtures are documented in `TASKS_EDITOR_PLAN.md`: XtremeJade v165, ForsakenJD v172,
Elite Jade Dynasty - HDN v184 client, and the 1792 v184 server set. Treat every fixture as read-only.

The user also edits ForsakenJD's task set in the app, so fixture tests treat its original root
count as a minimum. Never write into these folders from tests. Tests save into `std::env::temp_dir()`.

## How to verify changes

1. `cargo test --lib` (add a test for Rust behaviour, ideally against a real sample, skipping
   when it is missing). For a one-off investigation, a temporary `#[cfg(test)] mod probe_…`
   in `mod.rs` run with `--nocapture` works well. **Remove it afterwards.**
2. `npx tsc --noEmit` and `npx vite build`.
3. For UI, a mocked page in the browser preview: create `.preview/<name>.html` + `.tsx` that
   render the component with `window.__TAURI_INTERNALS__ = { invoke: async (cmd, args) => …,
   transformCallback: () => 0 }`, serve with Vite (`node node_modules/vite/bin/vite.js .
   --port 5179`), open `/.preview/<name>.html`, check it, then **delete `.preview/`**. Don't
   leave the shell's working directory inside it (Windows then can't delete it).
4. Update `README.md` for anything user-visible.

## Pitfalls we hit (avoid repeating)

- **`String.prototype.replace` with a string replacement interprets `$`** (`` $` ``, `$'`,
  `$&`). A README patch containing `` `!$` `` once pasted the README's opening into its
  middle. Use a function: `s.replace(a, () => b)`.
- Shell heredocs and `node -e` mangle backslashes (`\\s` became `s` in a regex) and
  apostrophes. For anything with regexes, backslashes or quotes, use a file-editing tool or
  write a script file to a temp folder instead of inline shell.
- Don't conclude "the data just doesn't have it" before checking the parser. The
  description bug above was first explained away that way.
- `Document::reload` (after schema edits) re-reads the same bytes with a new catalog and
  must carry over `edits`, `resources`, `disk` and `backed_up`; new `Document` state needs
  the same.
- Async Tauri commands lock `AppState.document` (a `Mutex`); keep work under the lock short
  and never lock `document` after `compared` (lock order: document, then compared).
- A task pack may contain at most 300 top-level roots. Root cloning searches existing packs for
  capacity; do not silently create a new numbered pack until that workflow is designed and verified.
- Saving a newly cloned or deleted top-level task intentionally clears task undo history after the
  staged set passes full reopen validation (root positions changed). A saved clone can now be removed
  with Delete quest.

## Open ideas / next steps

- JSON import (Tools › Import JSON) updates existing records. New JSON exports wrap
  `records` with `elementsVersion`, `formatVersion`, and list metadata; `_raw` preserves complete
  record bytes. These exports can add missing IDs in bulk only when versions and list layouts
  match. Legacy arrays remain update-only. IDs are preserved, additions check ID-space
  conflicts, invalid rows are skipped entirely, and preview tokens cover input, data and schemas.
- Pick-aware export (export only the picked search results).
- Task compare/translation/JSON workflows described in
  `TASKS_EDITOR_PLAN.md`.
- More data files in the activity bar (`gshop.data`, …). The bar shows a label under each icon
  (`--activity-width` in App.css).
- v165: the 8 bytes before list 296 that layouts mark as a checksum slot look like an empty
  list header (record size 1468, count 0). Check whether the layout should treat them as a list.

Shortcuts, for reference (tasks: Ctrl+Shift+F advanced search, Ctrl+Shift+M problems, Ctrl+H history): Ctrl+O open, Ctrl+S save, Ctrl+Shift+S save as, Ctrl+G find,
F3/Shift+F3 next/previous hit, Ctrl+Shift+F advanced search, Ctrl+Shift+M problems, Ctrl+H
history, Ctrl+L list picker, Ctrl+D clone, Del delete, Ctrl+Z/Ctrl+Y undo/redo, Ctrl+W close
tab, Ctrl+Tab/Ctrl+1–9 tabs, Alt+← back.
