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
cd src-tauri && cargo test --lib   # Rust tests (142 at last run; real fixtures can take longer)
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
| `tasks/container.rs` | Strict `tasks.data` index and numbered-pack reader: offsets, pack limits and MD5 validation. |
| `tasks/schema.rs`, `structures.rs`, `v165.rs`, `v172.rs`, `v184.rs` | Byte-preserving task schema engine and verified version layouts. |
| `tasks/browser.rs` | Lazy tree loading, background search/reference index, field inspection and hierarchy summaries. |
| `tasks/edit.rs` | In-memory task value edits, history, undo/redo, clone, delete and move/reparent operations. |
| `tasks/save.rs` | Task-set staging, pack rebuilding, MD5/index updates, verification, backups and atomic replacement. |
| `tasks/analyze.rs`, `layout.rs` | Unsupported-version analysis and validated user task-layout patches. |

### UI (`src/`)

- `App.tsx` — app shell: menus (File/Edit/Tools), panels, tabs, shortcuts, edit/save flow.
- `elements/api.ts` — one function per Tauri command. `elements/types.ts` mirrors the Rust
  structs (serde `camelCase`). **When you add a Rust command, add it to
  `generate_handler!` in `lib.rs`, to `api.ts`, and its types to `types.ts`.**
- `components/` — one component per panel/dialog (`AdvancedSearch`, `ProblemsPanel`,
  `ComparePanel`, `HistoryPanel`, `SaveDialog`, `SchemaEditor`, `RecordInspector`,
  `FieldTree`, `InlineEditor`, `PathDataEditor`, `TasksEditor`, task hierarchy dialogs, …).
- `schema/model.ts` — schema editor draft model; `schema/fieldList.ts` — pasted
  sELedit/Jade Editor field lists → fields.
- `elements/money.ts`, `time.ts`, `text.ts`, `talk.ts` — display helpers.
- `tabs.ts` — tab reducer (tabs follow records when rows shift).
- `App.css` — all styles, sectioned by feature. Colours are CSS variables (`--accent`,
  `--warn`, `--danger`, `--bg-*`, `--text-*`), light and dark.

### Data and tools

- `src-tauri/formats/layouts/<id>/` — built-in layouts (`layout.json` + `list_N.json`):
  v66, v112, v156, v156-signin, v158, v160, v165, v176. Embedded at build time.
- `src-tauri/formats/enums/`, `masks/` — built-in named sets.
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
  packs while preserving IDs. Top-level deletion is not implemented.
- Saving a top-level clone rebuilds its destination pack's offset table and MD5 and increments the
  index root count. Because the saved clone becomes a base root and root removal is unavailable,
  a successful structural save starts a new task undo history.
- `cloned_top_level_task_rebuilds_the_pack_table_and_index_count` covers the structural save/reopen
  path. The full Rust library suite, TypeScript check and Vite build passed after this feature.
- On October 8, 2026 the user cloned and saved a real v165 top-level task, then confirmed that both
  the matching server and client started successfully without a crash.
- The next planned feature is the task problems scanner and reference graph. Reuse the completed
  background search/reference index rather than decoding every root again.

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

Never write into these folders from tests. Tests save into `std::env::temp_dir()`.

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
- Saving a newly cloned top-level task intentionally clears task undo history after the staged set
  passes full reopen validation. Undoing that saved structural change would require root removal.

## Open ideas / next steps

- JSON import (Tools › Import JSON) updates existing records. New JSON exports wrap
  `records` with `elementsVersion`, `formatVersion`, and list metadata; `_raw` preserves complete
  record bytes. These exports can add missing IDs in bulk only when versions and list layouts
  match. Legacy arrays remain update-only. IDs are preserved, additions check ID-space
  conflicts, invalid rows are skipped entirely, and preview tokens cover input, data and schemas.
- Pick-aware export (export only the picked search results).
- Task problems scanner and reference graph, then compare/translation/JSON workflows described in
  `TASKS_EDITOR_PLAN.md`.
- More data files in the activity bar (`gshop.data`, `dyn_tasks.data`, `task_npc.data`, …).
- v165: the 8 bytes before list 296 that layouts mark as a checksum slot look like an empty
  list header (record size 1468, count 0). Check whether the layout should treat them as a list.

Shortcuts, for reference: Ctrl+O open, Ctrl+S save, Ctrl+Shift+S save as, Ctrl+G find,
F3/Shift+F3 next/previous hit, Ctrl+Shift+F advanced search, Ctrl+Shift+M problems, Ctrl+H
history, Ctrl+L list picker, Ctrl+D clone, Del delete, Ctrl+Z/Ctrl+Y undo/redo, Ctrl+W close
tab, Ctrl+Tab/Ctrl+1–9 tabs, Alt+← back.
