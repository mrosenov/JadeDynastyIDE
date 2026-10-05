# JD IDE

Desktop editor for Jade Dynasty (Zhu Xian) data files, built with Tauri 2
(Rust backend, React + TypeScript UI).

Reads, edits and saves `elements.data` across versions.

## How a file is read

1. **Layout.** Every built-in layout for the file's version is tried. A
   layout's marker table says where the checksum, exporter and tag blocks sit
   between lists. The file must fit it exactly, ending in the NPC dialog block
   at EOF. If several fit (server builds that diverged under one version
   number), the one whose record sizes match best wins.
2. **Other marker tables.** For unknown versions, the marker tables of other
   versions are tried, nearest first.
3. **Detection.** As a last resort, segments are recognised by content.
   This cannot tell a raw checksum slot from an empty list (v165 has one before
   list 296), which is why marker tables come first.

Each list then gets a definition:

| Fit | Meaning |
|---|---|
| Exact | the file's own layout, same record size |
| Partial | the file's own layout, records are larger (tail shown as unknown) |
| Borrowed | another version's definition, same record size |
| Grown | another version's smaller struct, paired by position between matches |
| Name only / Unknown | raw int32 view |

Borrowing aligns record sizes (a longest common subsequence within each
marker group), so lists inserted mid-way do not shift every later name.
Fields with enums/masks show their labels; fields that hold another list's IDs
link to that record. Texts show their line breaks (CR LF, or `!$` from the official
editor's exports) as ↵. Selecting a text with breaks or `^RRGGBB` colour codes opens a
preview that renders it as the game would, with the raw codes one click away.
Field rows alternate subtle background shades to make wide records easier to follow.
Display groups, arrays with more than one element, and nested struct rows use shaded, bold headings so their sections are easy to scan.
The schema editor can give either heading an optional colour; the colour stays on the heading only.

## Settings and the game client

Settings (gear icon, top right) take the game client folder: the client root or its `element`
folder. The Appearance setting follows the Windows theme by default or forces the light or dark
theme. The discovered data-file and package lists stay collapsed until clicked. JD IDE then:

- lists the client's data files (`element\data\*.data`) and opens `elements.data` from there.
  Other kinds (tasks, gshop, npcgen, …) are listed for later versions. It can also open
  the client's `elements.data` on start.
- reads `path.data`, so path and icon fields show their resource path.
- reads item icons from `surfaces.pck` (`surfaces\iconset\iconlist_ivtr.dds` + `.txt`).
  Icons show in the record table, the inspector, icon fields and tabs.
- reads the string tables of `configs.pck` (on first use, each once):
  - `item_ext_desc.txt`: item descriptions, shown below the bytes in the inspector (with
    `monster_desc.txt` for monsters and `addon_str.txt` for addons).
  - `item_color.txt` with the palette in `item_desc.txt`: item names in their game colour in
    the record table and the inspector (blended toward the text colour so they stay readable).
  - `skillstr.txt` and `buff_str.txt`: fields with the `skill` or `buff` role show the skill's
    (entry ID × 10) or buff's name. Hovering that name shows the skill introduction and
    detailed description (entries ID × 10 + 1 and +2, whichever exist) or the description
    below the buff name. The dark game-style popover renders the tables' `^RRGGBB` text
    colours, including white, independently of the app theme. `tools/type-rules.json` gives
    the role to ID fields such as
    `skill_id_1`, `id_skill`, `cast_skill`, `buff_id`.
- reads `Interfaces\script\config\title_def_u.lua` from `interfaces.pck`. Integer fields
  assigned the `title` role show the title name beside its ID; hovering the name shows its
  description in the same dark, colour-aware popover. JD IDE parses only the `id`, `note`
  and `desc` table fields and does not execute the Lua script.

  The tables are UTF-16 string tables (`#_index`, `#_begin`, quoted strings, `\r` for line
  breaks), read as the client's CECStringTab does (`src-tauri/src/client/strings.rs`). A quoted
  string can continue across physical lines, which is common in skill descriptions. If another
  numbered quoted entry begins before the quote closes, the previous row is treated as broken;
  this keeps Forsaken's three malformed `item_ext_desc.txt` rows from consuming the rest of the
  file.

Settings are stored in `%APPDATA%\com.jdide.app\settings.json`.
The optional **AI layout analysis** section is collapsed by default and stores an API endpoint,
model and API key there as well. The AI method inside **Analyze** is enabled after all three are
set. A URL ending in `/responses` uses the OpenAI Responses API; other base URLs use their
OpenAI-compatible `/chat/completions` endpoint. The key is sent only to that configured endpoint.

`src-tauri/src/client/pck.rs` reads Angelica File Packages (versions 2.2 and 2.3, including `.pkx`
continuation parts). `dds.rs` decodes DXT1/3/5 and uncompressed DDS, one icon's blocks at a
time. Icons are served to the UI as PNGs through the `jdicon://` protocol. Path IDs only
match when `elements.data` comes from the same client as `path.data`.

## Referenced by

The inspector's **Referenced by** tab lists the records that point at the selected
record's ID, grouped by list. Click one to open it (Ctrl+click opens it in a new tab).
References come from two sources:

- **ref:** fields whose `refs` name the record's list (exact).
- **by ID:** integer fields whose name says they hold an ID (`id_goods`, `id_to_make`,
  `item_id`, …) and whose ID space matches. IDs are only unique within a space, so
  task, skill, recipe or config ID fields are not matched against items. This is
  inferred from names, so treat it as a strong hint rather than proof.

## Tabs

Records open in tabs above the inspector:

- Clicking a record previews it in a reusable tab (shown in italics). Double-click or
  Enter keeps it open.
- Links navigate inside the current tab (Alt+← goes back). Ctrl+click or middle-click
  opens a link in a new tab.
- Ctrl+Tab and Ctrl+Shift+Tab switch tabs, Ctrl+1…9 jumps to a tab, and Ctrl+W or
  middle-click closes one.
- Open tabs are remembered per file.

## NPC dialogs

**NPC Dialogs**, first in the list picker, holds the `talk_proc` block at the end of the
file. A dialog is a tree of windows: each has the NPC's text and options that open a child
window or run a function (Back, Exit, and the NPC services of `SERVICE_TYPE` in ExpTypes.h).

- **Conversation** plays it like the game: pick options, go back along the trail, start over.
  Texts show their line breaks and `^RRGGBB` colours.
- **Outline** lays out every window as a tree, with windows no option reaches listed apart.
  Click one to continue the conversation from there.
- **Opened by** lists the records whose `id_dialog` points at the dialog (talk, shop, heal,
  teleport services…). In those records, `id_dialog` links to the dialog.
- **Edit text** opens a translation view with the original beside the editable title/prompt,
  NPC window texts and player option labels. A dark game preview applies line breaks and
  `^RRGGBB` colours. Fixed 64-character titles and options show their UTF-16 usage. Dialog IDs,
  window links, functions, parameters, counts and order stay locked; Apply text is one undoable
  history action.

Dialog tabs work like record tabs (Alt+← goes back to the service you came from).

## Lists and records

The records column starts with the list picker: the open list, and a dropdown (Ctrl+L) to search
all lists by name, struct or number (↑ ↓ Enter), with **NPC Dialogs** first and *Hide empty*. The
dot shows how the layout fits, italics a borrowed one, a pencil your own schema. Below it, the
search filters the list's records by ID or name.

## Editing

Values are edited in the inspector. Edits stay in memory until the file is saved (see *Saving*).

- **Double-click** a value (or press **Enter**/**F2** on the selected field) to edit it in place:
  numbers in decimal or `0x` hex, enums from a dropdown of their values (with *Other value…* for
  ones the enum lacks), bools as true/false. Enter saves, Esc cancels.
- **Masks**: the calculator (click the labels) has **Apply** for the ticked bits; in an enum's
  list, clicking a value sets it.
- **Smart value picker**: the search button beside a reference, skill, buff, title, path, icon
  or dialog ID finds valid values by ID or name and applies the selected value. References stay
  inside the field's declared lists or inferred ID space; client resources use the folder from
  Settings and can preview their coloured description. Search is on demand and debounced; results
  are paged in groups of 80, so every match remains reachable without transferring whole lists to the UI.
- **Texts** open an editor with a live preview in the game's colours, a length counter
  (`wchar[n]` holds n UTF-16 units, `char[n]` n GBK bytes; a line break counts as two, as
  CR LF), colour swatches and a picker that insert `^RRGGBB`. Ctrl+Enter saves.
- Values are checked against the field's type in that record (conditional types applied):
  integer ranges, floats, and texts that fit their fixed-size field. Text may use the final slot
  without a terminator, matching official data; the Problems scan still warns about these values. Bytes no layout describes edit
  as int32.
- Changed fields get an amber bar and show their original value on hover; changed records get a
  dot in the table, their tab and (with a count) the list picker; the status bar counts them.
  A record set back to its original bytes is no longer marked.
- **Edit** menu: *Undo*/*Redo* (Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z) name the edit they act on;
  *Revert record* / *Revert dialog* and *Revert all changes…* put records and dialog translations back as the file was opened or last saved
  (undoable too).
- **Edit › History** (Ctrl+H) lists every edit, newest first: when, what (e.g. *Set price*), the
  record and each field's old → new value. **Revert** puts that edit's fields back to the values
  they had before it; the edit then shows as *reverted* (with the time) instead of adding an edit
  of its own, so there is nothing to revert back and forth. Ctrl+Z takes a revert back. If later
  edits changed the same fields it asks before overwriting them. Clicking a record
  or field opens it there; undone edits stay listed (dimmed) until a new edit replaces them, and
  reverted ones are struck through. The unsaved change count in the status bar
  opens the history too.
- **Clone** (inspector button, Edit › Clone record or Ctrl+D) copies the open record to the end
  of its list with a new ID: the list's highest ID plus one, moved up past any ID another list of
  the same ID space already uses (items, types and services share one; addons, recipes and configs
  have their own), since a shared ID would hide one record from the game. The clone opens in a tab and is marked *new* (a green dot).
- **Delete** (inspector button, Edit › Delete record… or the Delete key in the record list) asks
  first and lists the records whose fields point at the record's ID (from *Referenced by*), since
  they would point at nothing afterwards.
- Clones and deletes are edits like the rest: undo, redo, the history (where reverting a delete
  brings the record back) and *Revert all changes*, which restores the file exactly as opened (or
  last saved).
  Records are tracked by identity, not row, so edits, markers and tabs follow records when rows
  move.
- Schema edits keep the record edits; opening another file or closing the app asks first: save,
  don't save or cancel.

## Saving

**File › Save** (Ctrl+S) writes the open file, **Save as…** (Ctrl+Shift+S) another one. The file is
written the way the official tools write it (`elementdataman::save_data`): the data as edited, the
header's export time set to now, and a new **checksum**. The game client checks it when it loads
elements.data and refuses to start on a mismatch:

```text
MD5( "ZPWDATA" + path.data + elements.data without its 8-byte checksum slots )
```

stored as 32 hex characters, 8 in each of the first four slots. Only those four are left out of
the hash: the 8 bytes v165 layouts mark before list 296 are hashed like the rest of the data. The path.data must be the one the client ships with this elements.data; it is
taken from next to the file, else from the client folder in Settings, or picked in the dialog.

The first save asks first, showing:

- the target, whether it replaces a file, and the changed / added / deleted records and translated dialogs;
- the checksum: whether the file on disk is valid with that path.data. Files saved by tools that
  leave the checksum alone show a mismatch (their clients do not check it); the saved file gets
  the right one either way. Without a path.data the old checksum stays and a client that checks
  refuses the file;
- a warning when another program changed the file since it was read;
- **Keep a backup**: the replaced file is copied to `elements.data.YYYYMMDD-HHMMSS.bak` next to it,
  once per file and session (remembered).

Later Ctrl+S saves right away with the same choices (the top bar confirms it with the time and
checksum), unless the file changed on disk, which brings the dialog back. The file is written
next to the target and then moved over it, so a failed save leaves the old file whole.

After saving, edits count from the saved file: markers clear, *Revert all* goes back to it, and
the history shows a *Saved* line where the save was made. Undo still goes back past it (the
file then differs from the saved one again). A dot next to the file name in the top bar shows
unsaved edits; clicking it saves.


## Menus and tools

**File** (Alt+F) in the top bar holds: *Open elements.data…* (Ctrl+O), *Save* (Ctrl+S) and *Save
as…* (Ctrl+Shift+S), the tools *Advanced
search* (Ctrl+Shift+F), *Problems* (Ctrl+Shift+M, with error and warning counts), *Compare with
another file…*, *Layout coverage* and *Settings…*.
**Tools** (Alt+T), beside File and Edit, holds *Export › Selected item… / Selected list…* and
*Import JSON…*. Import and export use JSON only.
A tool panel takes the place of the list and record panes; the shortcuts toggle it, and
the elements.data entry of the bar on the far left goes back to the lists. Tools keep their state
(results, scans) while hidden. After a scan, the status bar shows the problem counts.

The bar on the far left lists the game data files: elements.data now, others (tasks.data,
gshop.data, …) later.

## Compare files

**File › Compare with another file…** opens a second elements.data next to the open one: another version, or the
server's file against the client's. Lists pair by struct name (v160 and v165 line up although
their list numbers differ), else by name; records pair by ID; fields pair by path
(case-insensitively), else by the same offset and size.

- Per list: records added, removed and changed, and the record size if it changed.
- A changed record shows its fields before and after.
- The arrow between the files swaps which one is older ("before").
- **Copy patch notes** copies every difference as Markdown.
- Records of the open file open in the inspector; ones only the other file has are listed.
- Expand a changed record, pick one or more compatible fields, then **Copy selected** to take their exact values from the compared file into the open file. **Select all copyable** can take every compatible field and missing record shown in that list at once. Copying is one undoable history entry and remains in memory until the open file is saved.
- A record found only in the compared file can be copied into the open file when the elements versions and that list's record layout and field schema match exactly. The original ID is preserved; an ID already used in the destination ID space stops the copy. Whole records are never copied between incompatible versions or layouts.

Copy always goes from the **compared file into the open file**, independently of which side is shown as Before or After.

## Export

**Tools › Export** saves the selected record or the selected list, and **Export** in the search
results saves every match (not only the 500 shown), as JSON. Each record is an object of its scalar fields by path
(`addons[2].id`), after `_list`, `_listName` and `_row`. *Add enum and mask labels* adds a
`field#label` property.
JSON wraps the records in an object with the elements version, compatible list layouts, and
complete source bytes (`_raw`) for each record, including unknown fields. Use JSON to transfer
records between elements files.

## Import JSON

**Tools › Import JSON…** imports records from a JD IDE export. Choose a file to preview
**Added / Updated / Skipped** records and field changes (current → imported). **Apply** makes
all valid additions and updates as one undo step and one history entry. Edits stay in memory
until you save elements.data.

New JSON exports can transfer any number of missing records in one import, without choosing
templates. A missing ID is added at the end of its list using the complete source bytes, then
any edited field values are applied. Existing IDs receive updates to the included field values;
their unknown bytes stay unchanged. The imported IDs are preserved. Additions that collide with
another list in the same ID space, or with each other across such lists, are skipped.

The JSON includes version metadata around the records array:

```json
{
  "format": "jdide-elements",
  "formatVersion": 1,
  "elementsVersion": 165,
  "lists": [],
  "records": []
}
```

The exporter fills `lists` with layout metadata and `records` with the selected records. The
**elements version must match**: importing v165 JSON into an open v160 file stops with an error
before any changes. The record sizes, structures and field schemas must also match, since
server variants can share a version number. JD IDE does not convert between versions; use
compatible converted data and export again. Changing the JSON version number does not convert
the records. Keep the layout metadata and `_raw` values unchanged.

Older JSON exports (a bare array without version metadata) remain **update only**.
Export again as JSON to add missing records.

- Records match by **`_list` and the record's ID field** (`id` or `ID` as exported), never by
  `_row`. Keep IDs, `_list` and `_listName` unchanged, and import into the same layout used for
  export. `_listName` must match the target list. Ambiguous matches are skipped. Missing IDs
  are added only from new versioned JSON exports.
- Keep only the fields you want to update, using their exact exported paths such as
  `addons[2].id`. Omitted fields stay unchanged. `_row` and `#label` properties are ignored.
  Use the exported numeric values for enums and masks; editing their label properties has no effect.
- Empty text clears the text field; empty numeric values and JSON `null` are errors.
- Values must fit the field's type, including text length and encoding. Conditional fields use
  the imported value of their controlling field. Any error skips the **whole input row**;
  other valid rows can still be applied. Duplicate list/ID pairs in the import are all skipped.
- JSON uses the versioned object produced by Export (older record arrays are also accepted).
  Preserve large integers exactly when editing; numeric values can also be written as strings.
- The preview shows the first 200 additions, 200 field changes and 100 skipped rows, with full
  totals. Input row numbers start at 1 in the records array. If the import file, open data
  or schema changes after preview, **Refresh preview** is required before applying.

## Layout coverage

**File › Layout coverage** shows, per list, how many record bytes named fields describe, how many are
placeholders (`unknown_12`, `Unk3`, `pages_1_goods_2_unknown_4`…) and how many no field covers,
plus the share over the whole file (weighted by record bytes). Sort by least covered or by most
undescribed bytes to see where schema work pays off; the braces button opens a list in the
schema editor.

## Advanced search

Ctrl+Shift+F (or File › Advanced search) opens the search in place of the list
and record panes; it keeps its results while closed.

- **Conditions** on named fields: `= ≠ < ≤ > ≥`, *is one of* / *is none of* (`7, 8, 15`),
  *has flags* / *lacks flags*, *contains*, *starts with*, *ends with*, *is empty*. Match all
  conditions (lists without one of the fields are skipped) or any. Pick a field from the
  searchable field menu, which follows the selected list scope; a dotted path (`addons.id`)
  picks a struct member. Fields inside arrays
  match when any element does (for ≠, *is none of* and *lacks flags*: when every element does).
  Values can be numbers, `0x` hex, or the labels of the field's enum or mask
  (`Cannot be traded | Quest item`).
- **All fields**: choose one list (including the list currently open) or all lists, then search
  every decoded field of every record. Automatic mode treats the entered value as text and, when
  possible, as a number; integer, float, text and hex-byte modes are also available. Numeric modes
  can optionally search bytes no layout describes.

Results are grouped by list and show the matching field. Clicking one opens the record with
that field selected (Ctrl+click: new tab). *Copy IDs* copies the IDs of the picked results (or all shown), one per line.

## Problems

Ctrl+Shift+M (or File › Problems) scans the
whole file:

| Severity | Check |
|---|---|
| Error | **Duplicate IDs in a list** |
| Warning | **IDs hidden by another list**: records of different lists in the same ID space share an ID. Client, server and editor keep one ID → record map per space, so the record loaded later hides the other from ID lookups. Spaces follow the file loader's `elementdataman::setup_hash_map`: types and services share the item space, recipe types the recipe space, and WAR_ROLE_CONFIG plus ITEM_TRADE_CONFIG live among the items. A custom schema name can keep the recipe space by using `RECIPE` as a separate underscore-delimited part, such as `CRAFTING_RECIPE` |
| Error | **Broken references**: a field whose `refs` name a list holds an ID that list does not have |
| Error | **Missing dialogs**: `id_dialog` names no NPC dialog |
| Warning | **Dialog options** opening windows the dialog does not have |
| Warning | **Paths** (`path`/`icon` role) missing from the client's path.data (needs the client folder) |
| Warning | **Texts** filling their whole field with no terminator |
| Warning | **Values** their enum does not name |
| Info | **Mask bits** set but not named (all bits set, "every class", is fine) |
| Info | **Lists** read with a borrowed, partial or no layout |

Enum and mask findings are reported once per field and value, with how many records have it.
Problems are grouped by kind; the severity buttons and the filter narrow them down. Clicking one
opens the record with the field selected (dialogs open in the dialog viewer, list problems open
the list). Each kind keeps its first 1000 problems.

### Bulk edit

Results have checkboxes (each list's header picks its results, the bar above them picks all shown).
**Bulk edit** changes one field of the **picked** results, or of **all** results (every match, not
only the 500 shown); the dialog switches between the two:
*set to* a number, label or text; *add*, *subtract*, *multiply by*; or for masks *add flags* /
*remove flags*, which only touch the given bits (`proc_type += Cannot be traded`). The field is a
name (`proc_type`) or, when a record has it more than once, a path (`addons[2].id`). A live
preview shows how many records change, already hold the value, can't take it (it does not fit
the field; they are left as they are) or are skipped (their list has no such field), with old →
new samples and their labels. Applying is one undo step and one history entry.

## Find

Ctrl+G (or the box in the top bar) finds records in every list: by ID
when the query is a number, and by name (case-insensitive) always. ID matches come first,
then exact names, names starting with the query, and the rest. Enter opens a result in the
current tab, Ctrl+Enter in a new one. Afterwards F3 / Shift+F3 step through the same results.
IDs are only unique within an ID space, so one ID can match an item, an addon and a config.

## Built-in layouts (`src-tauri/formats`)

| Layout | Lists | Source | Checked against |
|---|---|---|---|
| v66 | 89 | marker table only | 1559/1792 `c01/elements.data` |
| v112 | 107 | Jade Editor names, 4 typed lists | JadeEditorFOX test file |
| v156 | 193 | server sources (`zx_source`), all typed | zxserver, Desktop copy |
| v156-signin | 194 | server sources (`JD1447`), extra `SIGN_IN_CONFIG` | none (no file) |
| v158 | 230 | Laravel jdide, 8 typed | 1559 server |
| v160 | 246 | JD IDE curated, 246 typed | Clean/1601 server, ForsakenJD client |
| v165 | 318 | Laravel jdide, 186 typed | 1792 server, Elite JD client |
| v176 | 294+ | Laravel jdide, 211 typed | none (list count unverified) |

Each layout is a folder, `layouts/<id>/`:

```
layouts/v165/
  layout.json     { id, version, source, markers: [{ before, kind }], listCount }
  list_0.json     the definition of list 0
  list_222.json   …one file per defined list (missing = not defined)
```

At startup only `layout.json` and each list's name, struct and size are read. A list's fields
are parsed the first time it is used.

Enums and masks are shared by every layout, one file per set:

```
formats/enums/gender.json          { key, label, values: [{ value, label, description? }] }
formats/masks/trade_behavior.json  { key, label, flags:  [{ bit, label, description? }] }   bit = 0…63
```

A field refers to a set by key (`"e": "trade_behavior"`). The v112 layout (from the Jade
Editor) uses the shared sets too. Yes/no fields use the `bool` enum: the source declares them
`int`/`unsigned int`, so they stay 4-byte integers. The official element editor's (ZElementData) mask tables are merged in: where an
existing mask covers the same bits, its flag descriptions gain the official Chinese label;
the tables with no counterpart are the `zx_*` masks. Edit them with **Enums & masks** in the schema editor, or with the pencil in a
field's value popover. Your changes go to `%APPDATA%\com.jdide.app\enums\` and `masks\`.
There, a file overrides the built-in set with the same key or adds a new one. Deleting
a built-in set writes `{ "key": …, "deleted": true }` there, which hides it; it is listed under
**Deleted** to restore. Removing a user file reverts the set. In the inspector, clicking a mask field's labels opens a
calculator: tick bits to get the resulting value in decimal and hex.

Fields can also have a display role (`"display"` in a list file, **Role** in the schema
editor). Most roles apply to integers; `probability` applies to floats:

| Role | Value | Shown as |
|---|---|---|
| `path` / `icon` | path.data ID | the client path (and item icon) |
| `skill` | skill ID | marked as a skill |
| `time` | unix seconds | `2014-03-28 12:26:34` (local; UTC in the tooltip) |
| `duration` | seconds | `1h 30m`, `7d`, `45s` |
| `duration_ms` | milliseconds | `1m`, `5s`, `250 ms` |
| `daytime` | seconds after midnight | `14:30`, `23:59:59` |
| `money` | Copper (100 Copper = 1 Silver, 100 Silver = 1 Gold) | `12G 34S 56C` (full words on hover); editing also takes `1G 50S` or `1 Gold 50 Silver` |
| `probability` | float ratio from 0 to 1 | `0.5` → `50%`; editing also takes `25%` and stores `0.25` |

The 0–1 probability scale is confirmed in the game source: common probability fields are
compared with random floats from 0 to 1, and several loaders reject values above 1. Some
arrays use the same values as relative selection weights whose total is 1.

`tools/type-rules.json` assigns roles by field name (`roles`), with units checked against
real values: medicine and revive scroll `cool_time` are milliseconds, recipe `cool_time` is
seconds (up to 7 days) although its source comment says milliseconds.
Each list definition is `{ name, struct, size, fields }`, and fields are `{ name, off, t, c?, e?, display?, refs?, g?, color?, gc?, when? }`.
Here `t` is a type tree (scalars, `wstr`/`str`/`bytes`, nested `array` and `struct`), and
`refs` names the target lists by struct, so a definition can be shared between versions.
`when` holds conditional types, e.g.
`[{ "field": "type", "in": [7, 8], "t": { "k": "f32" } }]`. The first rule whose sibling field
holds one of the values (or none of them, with `"not": true`) decides the type. The type must keep the
field size. Otherwise `t` applies.
An optional `g` puts consecutive fields into a named, collapsible display group
(collapsed by default). It never changes field names or offsets.
To add existing fields to a group, select the group's checkbox and the outside fields, then use **Add to _group name_**. Fields between the selected rows are included so their byte order and offsets stay unchanged.
Struct headings can carry an optional `color`; a group's optional colour is saved as `gc`
on its members. The schema editor manages both through the colour swatch on the heading row.

The schema editor (top bar, far right) saves each list you edit as
`%APPDATA%\com.jdide.app\layouts\<id>\list_<n>.json`. That file overrides that one list of
the built-in layout, and every other list keeps coming from the built-in layout. For a
version with no built-in layout, the editor also writes `<id>\layout.json` with the file's
marker table. "Revert to built-in" deletes the list's file.

**Import from… › Paste a field list…** turns a field list as sELedit and Jade Editor configs
write it into the list's fields:

```text
ID;Name;Type;Count;Value_1;Value_2;Value_3
int32;wstring:64;int32;int32;float;float;float
```

A line of names and a line of types (`;`, tabs or commas); a whole config block works too, and
its `001 - NAME` line fills in the struct name. Types: `int32`/`int`, `uint32`, `int16`,
`uint16`, `int8`/`char`, `uint8`/`byte`, `int64`, `uint64`, `float`, `double`, `wstring:N` and
`string:N` (N in bytes, so `wstring:64` is 32 characters), `byte:N`, `byte:AUTO` (the rest of
the record) and arrays such as `int32[4]`. The dialog previews each field's offset and size
against the record size, marks types it cannot read, and can cover bytes left over with
`unknown_XXXX` fields. The fields replace the draft; nothing is saved until Save.

**Analyze** helps migrate one list when a newer `elements.data` has a changed record layout.
It offers two methods. **Analyze from an older schema** runs locally: choose an older layout and
an `elements.data` that matches it exactly. JD IDE pairs records by ID, relocates stable known
fields from their byte values, and covers new or uncertain spans with `unknown_XXXX` byte fields.
**Analyze with AI** uses the same evidence, then asks the configured model to infer the added
fields' names and types. It sends only a bounded set of records for the selected list, not either
whole file. New fields may be inserted anywhere; neither method assumes a version only appends
bytes.

The returned definition must use the target's exact record size and pass local checks for field
counts, nesting, offsets, arrays and conditional types. It is also previewed against target
records. **Use proposal** only loads it as an unsaved schema draft, with the model's confidence
and uncertainties shown first. Review its fields and decoded record preview before pressing the
schema editor's Save button; analysis never changes `elements.data` bytes.

## Development

Prerequisites: Node 20+, Rust (stable, MSVC toolchain), WebView2.

```powershell
npm install
npm run tauri dev
```

Build an installer:

```powershell
npm run tauri build
```

Rust tests parse real files from `E:/` (override with the `JDIDE_SAMPLES`
environment variable). They skip any sample they cannot find.

```powershell
cd src-tauri
cargo test
```

## Regenerating layouts

Order matters: generate the source layouts first, then import the Laravel
structures. The import merges their enum/mask/ref hints into the source layouts.

```powershell
node tools/gen-elements-schema.mjs "E:/Game Dev/JD/zx_source/zgame/gs/template" v156 "E:/Game Dev/JD/zxserver/zgame/gs/config/elements.data"
node tools/gen-elements-schema.mjs "<extracted JD1447>/zgame/gs/template" v156-signin
node tools/import-jade-editor-profile.mjs "E:/Game Dev/JD/Tools/JadeEditorFOX/JadeEditorPython/formats/elements"
node tools/import-laravel-structures.mjs "C:/Users/mitko/Herd/jdide/resources/structures/elements" src-tauri/formats "E:/Game Dev/JD/1559/gamed/config/elements.data" "E:/Games/ForsakenJD/element/data/elements.data" "E:/Game Dev/JD/1792/gamed/config/elements.data"
node tools/import-official-sets.mjs "E:/Game Dev/ZX-Client-Src/ZElement/ZElementData"
```

Then apply `tools/type-rules.json`: its conditional types (e.g. addon `param1` is a float for
rate-like addon types), and its `sets`, which give integer fields without an enum/mask one
by name (`proc_type`, `equip_mask`, `sect_mask_1`…). A field takes the set its namesakes
in the same layout use, else the one most used across layouts, else the rule's own:

```powershell
node tools/apply-type-rules.mjs
```

Check how every layout of a file's version fits it:

```powershell
node tools/check-layouts.mjs <elements.data> [...]
```

The whole `src-tauri/formats` folder is embedded at build time, so a new layout folder
needs no code change.
