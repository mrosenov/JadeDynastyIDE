# JD IDE

Desktop editor for Jade Dynasty (Zhu Xian) data files, built with Tauri 2
(Rust backend, React + TypeScript UI).

Reads, edits and saves `elements.data` across versions, with separate tools for the
client resource table in `path.data` and static quests in `tasks.data`.

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

- lists the client's data files (`element\data\*.data`) and opens supported `elements.data`
  and `tasks.data` files from there. Other kinds (gshop, npcgen, …) are listed for later versions. It can also open
  the client's `elements.data` on start.
- reads `path.data`, so path and icon fields show their resource path.
- reads item icons from `surfaces.pck` (`surfaces\iconset\iconlist_ivtr.dds` + `.txt`).
  Icons show in the record table, the inspector, icon fields and tabs.
- previews standalone client images for fields with the `image` role. The value is a
  `path.data` ID such as NPC `Profile_Path_ID`; hovering the resolved path reads the image
  from its package and shows it at a useful size. TGA and DDS are converted to PNG on demand;
  PNG, JPEG, GIF, WebP and BMP are served directly. The image picker offers only paths that
  are actual standalone package entries, excluding atlas names and stale `path.data` paths.
  Hover changes the preview only; the row last clicked remains the value applied by **Use selected**.
  Atlas cells keep using the `icon` role.
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

## tasks.data browser

The stacked-file icon in the activity bar opens the static task browser. If Settings has a
game client folder, its `element\data\tasks.data` opens automatically. **Open…** can select a
different task index. Versions 165, 172 and 184 are currently supported.

Opening verifies the index, every numbered pack (`tasks.data1`, `tasks.data2`, …), every root
offset table and each pack's stored MD5 before showing any quests. It also reads each root's
direct-subtask count with a lightweight parallel schema scan. The header shows the task version,
root count, pack count, combined size and integrity result. Root and nested quests can be searched
by ID or name and are paged 200 at a time with a direct page-number box. A number lists the exact ID
first, then IDs starting with it and names containing it. Nested names and IDs are
indexed in the background, while root results remain available immediately.

Roots containing subtasks show a small **+** as soon as the file opens. Expanding or selecting a
root decodes only that complete quest tree; nested subtasks use the same tree controls. Selecting any task
shows its typed fields, source offsets and byte sizes, organized into General, Availability,
Prerequisites, Objectives, Failure, Rewards, Text and dialogs, and Hierarchy sections. Structures
and arrays are collapsible. Fields whose purpose is not known keep their fixed raw bytes; short
raw values also show unsigned, signed and floating-point interpretations on hover. Known task IDs
link to the matching quest, item and monster IDs link to the open `elements.data`, and known skill,
buff and title values use the configured client's names and descriptions. Until the background
index is complete, links to subquests show *Indexing quests…*; the open task refreshes once it is.
One decoded root is cached, so moving among its subtasks does not reread or decode the pack.

**Advanced search** (Tools › Advanced search or Ctrl+Shift+F) finds every quest, subquests included,
that has something specific. It opens in place of the quest list, beside the inspector, and keeps
its conditions and results while hidden. It reads the task set as it is in memory, so unsaved edits
count. A search takes a few seconds (searches for an ID are faster), shows its progress and can
be cancelled.

- **Conditions on fields**: rows of field, operator and value, matching all or any. The field list
  names fields as the form does (*Objectives › Monsters to kill › Monster ID*) and can be searched.
  **Any award** and **Any dialog** fields stand for the same field in every award (success, failure
  and each ratio, item-count and finish-count entry) or every talk, e.g. *Any award › Candidates ›
  Items › Item ID*. Named values (type, class, completion method, …) are dropdowns, recommended
  types are checkboxes with *has flags*, and flags are true/false. On a list, = and the other
  positive operators need one row to match; ≠, *is none of*, *lacks flags* and *is empty* must hold
  for every row.
- **On the same row**: a condition on a field of the same list as the one above can require the same
  row, e.g. a candidate reward with item 12345 *and* an amount above 5 in that one row.
- **Start from a common search**: Rewards item, Requires item, Gives item on accept, Collect item,
  Kill monster, Given by NPC, Completed at NPC, Requires class, Requires quest, Leads to quest, Dialog
  option gives quest, Quest type, Level between and Name contains fill in editable conditions.
- **A value anywhere**: a number matches number fields (IDs, amounts, …); other text is searched in
  names, texts and dialogs. *Only ID fields* limits it to quest, item, monster and NPC fields and
  dialog option parameters.
- **Look in** all quests and subquests, top-level quests only, or one quest and everything below it.

Each result shows the quest, whether it is a subquest (and of which quest), and its first matching
field and value (hover for all). Clicking opens the quest on the form tab that holds the match.
Results can be picked; **Copy IDs** and **Export** (task JSON) use the picked results, or all listed
ones. Up to 500 results are listed; the total is always counted.

**Problems** (File › Problems or Ctrl+Shift+M) scans the task set once every subquest is indexed and
shows its results in place of the quest list, beside the inspector. Clicking a problem selects the
task. The header button shows the error and warning counts, and every edit rescans at once because
the scan reads the background index instead of decoding the packs again.

| Severity | Check |
|---|---|
| Error | **Top-level tasks the game skips**: a top-level ID an earlier top-level task already uses. The loader keeps the first in pack order and skips the later task entirely ("Dup Task Found"). |
| Error | **Duplicate task IDs** among all tasks, subquests included. The game's map of all tasks keeps only the one loaded last. |
| Error | **Broken task references**: a prerequisite, exclusion, award (`new task id`, terminated tasks) or finish-count field naming a task ID no task has. |
| Warning | **Tasks that name themselves** in a prerequisite or exclusion list. |
| Warning | **Stale hierarchy links**: stored parent, sibling or first-child IDs that differ from the tree. |
| Warning | **Items and monsters missing from elements.data** (item, monster and object IDs), checked only while an elements.data is open. |
| Info | **Full task packs** holding 300 top-level tasks. |

The inspector's **Referenced by** section, below the fields, lists the tasks whose prerequisite,
exclusion, award or finish-count fields name the selected task. Click one to select it.

The prerequisite (`premise tasks`, `premise finish tasks`, `premise global task`, `premise cotask`)
and exclusion (`mutex tasks`) fields are named in the v165, v172 and v184 layouts from
`ATaskTemplFixedData`. Their offsets were checked on every task of the three client fixtures: counts
never exceed 5 and unused slots are zero. They link to their tasks, are validated when edited, are
remapped inside clones and are followed by delete previews, Problems and Referenced by. The four
hierarchy link fields are shown but locked, because clone, move and delete maintain them.

**Tools › Export** writes tasks as JSON: the selected task, the selected task with every subquest
below it, or the listed tasks (the search results, or every top-level task without a search). Each
task is listed by ID with its editable values by field path, such as `fixed.name`,
`fixed.premise_tasks[0]` or `fixed.time_limit`. Text keeps CR LF line breaks, floats use their
shortest exact form, and integers beyond 2^53 are strings. Raw fields of unknown meaning, the task
ID and the hierarchy links are left out. Top-level tasks also carry `_raw`, the complete bytes of
the task and all its subquests.

```json
{
  "format": "jdide-tasks",
  "formatVersion": 1,
  "taskVersion": 172,
  "schemaDigest": "…",
  "tasks": [
    { "id": 2500, "name": "Join Jadeon", "_pack": 0, "_root": 0, "_path": [], "fields": { "fixed.name": "Join Jadeon" }, "_raw": "c4090000…" }
  ]
}
```

**Tools › Import JSON…** previews **Added / Updated / Skipped** tasks with current → imported values
and applies them as one undo step; nothing changes on disk until you save. The task version and the
layout (`schemaDigest`) must match the open task set. Tasks match by `id`; `_pack`, `_root` and
`_path` are informational. Only the fields present in `fields` change, with the inspector's rules:
fields that control counts or structure, task IDs and hierarchy links are locked, array lengths
cannot change, text must fit, and task references must name a task that exists (or one added by the
same import). Any error skips the whole row. A missing top-level task is added from `_raw` with all
its subquests and their IDs, appended to its original pack when it has room, otherwise another; an
ID in its tree that the open set already uses stops that row. Missing subquests are not added.
Duplicate IDs in the file, or IDs several open tasks share, skip their rows. If the file or the
task set changes after the preview, **Refresh preview** is required before applying.

**Tools › Compare with another tasks.data…** opens a second task set read-only (another version, or
the server's set against the client's) and shows the comparison in place of the quest list, beside
the inspector. Tasks pair by ID. Roots whose bytes are identical at the same position are counted
without decoding, so comparing a set with a lightly edited copy is quick; comparing two versions
decodes every task and can take half a minute. The panel shows Before and After (the arrow swaps
them), counts of added, removed, changed and identical tasks, and **Copy patch notes** copies those
lists as Markdown. A changed task lists its differing fields (dotted paths, as in JSON exports) when
expanded, and is marked when it has another parent or its subquests differ. Fields that exist only
in one version are listed only when they hold a value. IDs that several tasks share on either side
are not compared.

Tick fields, a whole changed task (every compatible field), or compared-only top-level tasks, then
**Copy selected** to take them from the compared set into the open one as one undo step; **Select
all copyable** picks everything that can be copied. Copying goes through the JSON import checks:
a field copies when the path and binary type match, so values copy between versions too; whole
tasks (with all their subquests and IDs) copy only between identical versions and layouts, and only
when none of their IDs is already used. The comparison refreshes after each copy.

**Tools › Translate from tasks.data…** copies human-facing task text from a translated task set
into the open one. The versions may differ: tasks pair by ID and texts by field path. Three groups
can be applied separately:

| Group | Texts |
|---|---|
| Names | task names and signatures |
| Descriptions | description, success, failure, tribute, hint and can-deliver texts, award tributes |
| Dialogs | NPC window texts, player option labels and talk prompts |

The preview shows per group how many tasks and texts change, with samples (current → translated),
and counts what is skipped: tasks the source lacks, blank source text (the open text is kept), text
too long for its fixed-size field, and talks whose windows or options differ in shape (translating
those could put text in the wrong window). Only text changes; IDs, numbers, counts and raw bytes
never do, and counted text keeps its stored length in step. Each text keeps the open file's
terminator convention: v165 dialog texts end with a NUL character inside the text, later versions
do not. Applying the chosen groups is one undo step, and only re-reads the task roots it changes. If
the open task set changes after the preview, preview again.

The inspector shows a quest as a form with tabs, like the official editors: **General** (identity,
time, flags, timetable, hierarchy and Referenced by), **Locations** (delivery and award NPCs, zones,
transfer, receivers), **Requirements** (level, class, quests finished first, mutually exclusive
quests, items, titles, costs, team and more), **Objectives** (completion method, monsters and items
to collect, interaction objects, sites, escort, failure conditions), **Rewards** (success and failure
awards and the scaled award tables) and **Texts & dialogs** (description, hints and every NPC talk
with its windows and options). Every group lists all of its fields in a compact multi-column grid;
flags are checkboxes, times and positions edit their parts in place, and lists and tables show item,
monster, NPC and quest links. Completion method, finish type, award types, task type, quest log
category (display type), dynamic type, repeat frequency, receiver limit reset, gender and co-task
condition are dropdowns (`[1] Kill monsters`), and **Recommended for** (recommend type) is a row of
checkboxes (Pinned, EXP, Cash, Affinity, Orb, Special item, Title, Gear). Names come from
`TaskTempl.h`/`DlgTask.h` and the English client's interface strings; task types 7 and 14 follow the
English client (Clan, Vitalic) where the C++ source says cultivation and cross-server, and 18–20 are
the white, green and red Archaia quests. **Difficulty** is the 1–5 star rank. Editing works as before (click a value, Enter applies) with the
same checks, undo and history. **Advanced** keeps the complete field tree with types and offsets.
The last tab is remembered. Labels share one column width so values line up, and item, NPC and quest
links stay on the value's line (long names are shortened; hover shows the full name).

Tables and lists (monsters to kill, items to collect or give, timetables, candidate items, quests
finished first, exclusive quests, failure monsters and items, …) are plain tables of input boxes:
type a value and press Enter or leave the box to apply it, or Escape to restore it. Each row has
**Clone** (inserts a copy below it) and **Remove**, and **Add row** appends an empty row. Lists that
grow with the quest change size and their count; fixed lists (for example the 5 slots of quests
finished first) fill their slots in order and show how many are used. Each row operation is one undo
step. Adding or removing given items also updates the stored common/task item counts.

**Texts & dialogs** edits the quest's five NPC talks, each on its own tab (a dot marks talks in use):
**Delivery** (offered when the quest can be accepted), **Unqualified** (shown greyed out when the
requirements are not met), **Item delivery**, **Execution** (while the quest is in progress) and
**Award** (when it can be completed). A talk is a tree, shown like the official editor: the first
window, its options, and under each option the window it opens. You can:

- edit the prompt (the NPC menu entry that starts the talk), window texts (several lines) and option
  texts (at most 63 characters, as the game stores them);
- choose what an option does: **Opens a window** (adds a new window under it, with a Back option) or
  an NPC function: Give quest, Complete quest, Give quest item, Give up quest, Talk, Back or Exit.
  For the quest functions the parameter is a quest ID with its link, filled with this quest; "use this
  quest" resets it;
- add options, move them up and down, and remove them. Removing an option, or turning it into a
  function, removes the window it opened and every window below it (after asking when they hold text);
- **Create talk** for an empty stage with a starter (Delivery: Accept → Give quest and Leave → Exit;
  Award: Complete → Complete quest and Leave; the others: Leave), and **Clear talk**;
- **Preview** the talk like the NPC window in game, with colour codes: options open windows, Back
  returns to the window before, and functions end the talk saying what the NPC would do.

Every change is one undo step. JD IDE writes talks the way the official editor does: the first window
(parent -1), then each option's window followed by its own windows, with parent IDs set and new
windows numbered after the highest ID. Line breaks are stored as CR LF and window texts keep the
file's terminator convention; v184 window parameters stay with their window, and a talk written
back unchanged stays byte for byte the same. Options naming a quest that does not exist are refused,
and a window no option opens (one HDN talk has one) is flagged with a button to remove it.

Cloning a quest moves the quest parameters of its options to the copy, so its NPC gives and completes
the copy, not the original. Option parameters also count as quest references in Problems,
Referenced by and the delete preview.

**Rewards** follows the official editors: a reward selector (award on success or failure, and the
ratio, item-count and finish-count awards for each), the two award-type dropdowns, and for scaled
awards their entries. A scaled award holds up to 5 entries, each an award with its time ratio, item
count or finish count: **Add entry**, **Clone entry** (with its value, right after it) and **Remove
entry** keep the entries and their values paired and the entry count in step, each as one undo step.
The panel says how the server picks an entry (ratio awards: the first whose ratio is at least the share
of the time limit used; item-count awards: the first whose count the player's item count reaches;
finish-count awards: from the last entry back, the first whose count the finish count reaches) and
warns when the values are not in the order that rule needs. The selected award is split into sub-tabs
(Dividends, Candidate items, Storage, Faction, Travel, Spawn, Quests, Character, Messages,
Friendship, Other); a dot marks sources and sub-tabs that hold values. Candidate items list each
candidate with its random-choice flag and an item table (item, common, amount, probability, bind,
period, timetable, day, time, refine, replacement).

**Classes** under Requirements is a table by race (Humans, Athans, Etherkins, Deikins) with one row
per class and a checkbox per tier (pre-tier, tiers 1–5) showing its class ID
(`CHARACTER_CLASS_CONFIG.character_class_id`, which task class requirements store). A class's
checkbox ticks or clears all of its tiers and a race's checkbox all of its classes; a partly selected
class or race shows a dash. Novice, GM and any other class the open elements.data defines are listed
under **Other classes**. Every change rewrites the class list and its count as one undo step, up to
the 45 slots a task has; no class selected means any class can take the quest. **Friendship** requirements and award friendships are listed by faction name. Class, friendship,
completion-method, finish-type, award-type and the other task value names are named sets
(`task_occupation`, `task_friendship`, `task_method`, `task_finish_type`, `task_award_type`,
`task_type`, `task_display_type`, `task_dynamic_type`, `task_avail_frequency`,
`task_clear_receiver_type`, `task_gender`, `task_cotask_condition`, and the mask
`task_recommend_type`) that can be edited in
**Enums & masks** (**Tools › Enums & masks…** in every workspace, or **Edit names…** in a task
dropdown; the form picks up the new names straight away); `task_occupation` names every class in the table (for example `Arden Tier 1` = 39); classes it does
not name show the elements.data name.

Almost the whole 2,490-byte v165 task header is named from `ATaskTemplFixedData` in
`TaskTempl.h`: offsets were computed from the declaration and match every previously verified field
and the total size. v172 adds the `kermis` flag after `faction` and v184 adds twelve bytes after
`premise_has_king`; both positions were found on the client fixtures. Only in-memory pointer
slots remain raw. About 1,070 quests per client store a minimum level above the maximum (for example
200 and 78); this is the game's own design, not a layout error.

**Task schema** opens the effective binary layout for the current task version. It lists every
structure, field type and condition and can search across all four. Verified v165, v172 and v184
layouts are read-only. For an unsupported version, the same window shows the selected older
baseline plus accepted user patch fields, which are marked separately; structural changes still
go through the analyzer so every operation is validated against the complete task set.

Click an editable value in the inspector to change it in memory. Known integers, floats, booleans,
names, quest text, existing array entries and dialog text are supported. Unknown fixed-width fields
accept exact hexadecimal bytes and retain their declared width. Fields that control counts,
conditions or task IDs stay locked so an ordinary value edit cannot change the record's binary
shape or invalidate task links (**Change ID…** next to the quest ID changes it safely, see below). Variable-length text updates its stored count automatically, and
every edited root is decoded and byte-round-tripped before the change is accepted.

Changed fields and roots are marked in green. Undo, redo and the edit history sit beside **Save…**
in the task editor's top bar and in the **Edit** menu (Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z, Ctrl+H);
the inspector toolbar keeps only actions on the selected quest. Undo restores the complete original
root bytes, including all offsets after variable-length text.

**Edit › History** (Ctrl+H), the history button or the *changed roots* count opens the task edit
history in place of the quest list and inspector, like the elements.data history. It lists every
edit newest first with its time, quest and old → new value, can be filtered by edit, quest name,
task ID or field, and shows a *Saved* line where the task set was saved. Clicking an entry selects
its quest, wherever it is now. **Revert** takes back one edit without undoing later ones; the entry
then shows *reverted* (with the time) instead of adding an entry of its own, and Ctrl+Z takes the
revert back. Because edits store complete task roots, Revert is offered only while no later edit
changed the same root; otherwise the button is disabled and its tooltip names the later edit.
Revert all is in the history header and the Edit menu. After undo, redo or revert all, the root list and tree follow
the restored structure and the same quest stays selected, wherever it is now. Switching tools
preserves the selected task, changed roots, history and undo/redo journal; JD IDE asks before
closing or opening another task set. The elements.data shortcuts (Ctrl+D, Ctrl+G, Ctrl+W, …) do
not act while the tasks.data workspace is shown.

Selecting a top-level task provides **Clone task**. It creates a complete new top-level task with
fresh IDs for the root and every descendant, remapping references inside the copy. It uses the same
pack when it has room, otherwise another existing pack with room. Selecting a subquest instead
provides **Clone subtree**, which copies it beside the original with the same fresh-ID and internal
reference handling. External task references remain unchanged. Fresh IDs start above every ID in
the task set and every ID cloned or deleted earlier in the session, so a deleted quest's ID is never
given to a different quest while references to it may remain.

Every quest stores the IDs of its parent, previous sibling, next sibling and first child in the last
16 bytes of its fixed block. The official editor refreshes them before saving and every quest in the
v165, v172 and v184 fixtures has them exact; the game recomputes them after loading. Clone, delete
and move rewrite these links for the whole affected root, so the saved tree matches what the
official editor would write. Roots whose links were already inconsistent are left as they are.

**Delete subtree** (a subquest) and **Delete quest** (a whole top-level quest with its subquests)
first check the completed background quest index and show every surviving task reference that would
become unresolved (references to an ID another surviving quest also has still resolve). The user must
explicitly confirm deletion when references exist. With an elements.data open, the dialog also lists
its quest-ID fields that name the deleted quests (NPC give and complete lists, task dice, mines,
interaction objects, …), all ticked: confirming clears them to 0, which the client's NPC quest lists skip, as one
undo step in elements.data (saved from the elements workspace). Cloning and deletion are each one
undoable edit and must pass an exact decode/encode check before being accepted.

A deleted top-level quest leaves the list at once, but keeps its place in its pack until the next
save, so undo, redo, revert and the history can bring it back exactly where it was. Saving rebuilds
the pack without it (the quests after it move up, an emptied pack is valid), lowers the index's quest
count, and, like saving a top-level clone, begins a new task undo history. Deleted IDs are not reused
by later clones. A cloned quest that has not been saved yet can be deleted only while it is the last
one added to its pack.

Top-level cloning and saving were validated on a real v165 task set on October 8, 2026: the
saved files started successfully in both the matching server and client without a crash.

**Move subtree** opens a searchable destination picker. It appends the selected subquest and every
descendant below the chosen existing quest, including across roots or packs, while keeping task IDs
and references unchanged. The old and new parent counts are rebuilt, moving into the selected
subtree is rejected, and the complete move is one undoable operation. If undo or another edit moved
either quest while the picker was open, the move is refused instead of acting on whatever quest now
sits at the old position.

**Change ID…** (next to the ID on the General tab) gives a quest, top-level or subquest, a new ID.
Enter the ID and **Preview** first. The preview lists:

- every quest reference that will be rewritten: prerequisites, exclusions, next quests, terminated
  quests, finish counts, team and master/apprentice quests, global and co-tasks, and dialog option
  parameters (Give quest, Complete quest, …). The quest's own references to itself change too, and
  the parent, sibling and child hierarchy links follow;
- every quest-ID field of the open elements.data that holds the old ID: the NPC give and complete
  lists (`NPC_TASK_OUT_SERVICE`, `NPC_TASK_IN_SERVICE`), `NPC_TASK_MATTER_SERVICE`, task dice
  (`TASKDICE_ESSENCE` quest lists), mines (`task_in`, `task_out`), interaction objects, battle and transcription rewards, buildings,
  `PIE_LOVE_CONFIG` and others. Each place can be unticked.

The new ID must not be used by any quest, and every quest must be indexed. The tasks.data change is
one undo step; the ticked elements.data places are one undo step there and are saved from the
elements workspace. When another quest has the same old ID (duplicate IDs), only references inside the
quest's own tree are rewritten and the elements.data places start unticked, since they may mean the
other quest. A freed ID is not reused by later clones. Players keep quest progress by ID, so changing
the ID of a quest on a live server affects players who have it active or finished, and other files
that name the quest (`task_npc.data`, server scripts) are not updated.

**Save…** or Ctrl+S writes the task index and its numbered packs. JD IDE rebuilds only packs with
edited roots when saving over the open task set, recalculates every affected root offset and pack
MD5, and preserves unchanged packs byte-for-byte. Save As writes a complete task set. Before any
replacement, the complete staged set is reopened, every pack checksum is checked, and every root
must decode and encode back to identical bytes. An optional timestamped `.bak` folder keeps the
complete replaced set. A changed-on-disk guard stops saving if the index or any source pack was
altered by another program; read-only destination files are made writable. Undo and redo remain
available after a successful save, except when the save adds new top-level tasks: those become part of
the saved set, so the save dialog warns that undo history starts again from the saved state.

If a task version has no verified layout, JD IDE opens a read-only **Task layout analyzer**
instead of treating the file as editable. The outer index, numbered packs, offsets and MD5 values
are still verified first. Choose v165, v172 or v184 as an older baseline and run **Analyze layout**
to test every root. The report separates byte-exact roots, roots where the baseline ends with
trailing bytes, and structural failures; it shows root and byte coverage, the first stopping
field and offset, and coverage for each pack. Analysis never changes the file, and saving remains
disabled until the effective layout round-trips every root exactly and the user accepts it.

The analyzer can also compare the newer file with an older supported `tasks.data`. Root tasks are
matched by ID rather than file order. The comparison counts matching, added, removed, renamed and
duplicate IDs, then groups matching roots by their old and new byte sizes. A repeated pattern such
as `8,468 B → 8,472 B` across thousands of IDs is strong evidence that a four-byte field was added
to the task structure. The report keeps a few example IDs for each size pattern and bounded examples
of ID/name differences, so large task sets do not create an oversized interface.

After comparing the files, **Find candidates** tests 1, 2, 4, 8, 16 and 32-byte insertions at
real field boundaries from the older schema. It samples matching changed roots and ranks positions
where bytes before the boundary still match while bytes after the proposed block realign. Each row
shows the containing structure, preceding field, width, likely fixed-width types, supporting sample
count, offsets and example raw bytes. **Add** turns one selected suggestion into a named fixed-width
field in a user patch at `%APPDATA%\com.jdide.app\task-layouts\v<version>.json`. Raw is the safe
default; the picker can assign a same-width integer, float, boolean or byte type, and the patch panel
can change that type later without moving following fields. JD IDE validates every addition or type
change, rechecks every root, then stores the patch. The patch panel can also remove an operation.
**Find next candidates** then removes the accepted fixed-width spans from temporary root copies and
ranks the remaining differences. Adjacent additions can anchor after a field already in the patch,
so layouts with several new fields can be built incrementally. The reference file must match the
patch's baseline version. Task data is never changed during analysis.

An inserted field can be **Always** present or have one or more conditions. Conditions use an
earlier integer field in the same structure and support zero/non-zero, equality, ranges, lists and
bit-mask predicates. Every condition must match before the field is read. Applying or clearing the
conditions validates the controller references, analyzes every root again and only then saves the
user patch. Iterative candidate scoring also evaluates those conditions per record before removing
accepted bytes from its temporary comparison copy.

**Counted array…** adds a variable-length array manually when fixed-width comparison cannot infer
one. Choose its containing structure and insertion point, name the earlier integer field that stores
the count, then select a scalar item type or reuse an existing task structure. The schema validator
rejects forward, missing and non-integer count references or unknown item structures, and whole-file
analysis runs before the operation is saved. Fixed-width candidate scoring pauses after a variable
field is present because its byte span differs per record; the coverage report remains available.

**Baseline field…** handles changes inherited from the older layout. **Replace type** keeps the
field name and conditions but assigns another fixed-width scalar, byte or raw type; the dialog shows
the old and new byte widths. **Remove field** deletes it from the effective schema. Both operations
are rejected when they break a later count, condition, insertion or structure dependency, and both
run whole-file analysis before being stored. Removing their row from the patch restores the baseline.

Use **Export…** to share or back up the complete versioned patch as JSON. **Import patch…** accepts
that JSON only when its target version matches the open `tasks.data`, validates every schema
operation, runs whole-file coverage, and replaces the active user patch only after those checks
complete. Importing a patch still does not enable task editing or saving by itself.

When coverage reaches an exact byte-for-byte round trip for every root, **Accept layout and open
editor** reruns that complete check, performs the normal task-browser decode, and stores a digest of
the exact user schema. The version then opens in the normal editable task workspace and is marked
**Accepted user layout**. Any later schema operation clears that acceptance. **Edit layout** returns
the version to the read-only analyzer so the schema can be changed and verified again. A matching
older baseline can also be accepted without adding operations when it already matches the newer
file exactly.

## path.data editor

The folder-tree icon in the activity bar opens `path.data` as a separate data workspace. If
Settings has a game client folder, its `element\data\path.data` opens automatically. **Open…**
can load a different client or server copy without changing the configured client.

The table searches by ID or path and edits both values inline. **Add path** chooses one above
the highest current ID; IDs may then be entered manually. Rows are paged 200 at a time, with
first/last controls and a page-number box for direct jumps. Delete
opens a confirmation dialog with the affected ID and path, and undo/redo covers field changes,
additions and removals. Saving validates the
whole table before writing: ID zero, duplicate IDs, duplicate paths, empty paths, characters
GBK cannot store and paths over the client's safe 255-byte limit are rejected. Rows are written
in ascending ID order like the official exporter. Save uses a temporary file, notices outside
changes and can keep a timestamped backup.

**Export JSON…** writes the complete table with its source path, JSON format version and export
time. **Import JSON…** validates the complete input, previews how many rows will be added,
changed or removed, then replaces the open table as one undoable edit. JD IDE exports and plain
arrays of `{ "id": number, "path": string }` rows are accepted.

The binary format is `u32 signature (0x504D4944)`, `u32 count`, then `count × { u32 id, u32 byte length,
GBK path bytes }`. It has no version, timestamp, terminators or trailing section. Because the
client includes the complete `path.data` in the `elements.data` checksum, save the matching
`elements.data` again after changing a path table. The editor repeats this reminder after save
when it finds `elements.data` beside the target. The binary file has no timestamp field; Windows
updates its file-modified time on save. The time stored in a JSON export is metadata only.

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
search filters the list's records by ID or name. Record rows show the ID and name without the
internal row index.

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
- **Quick edit**: click a number field to target it, or build a multi-field selection with the
  checkboxes or Ctrl+click. Choose set, add, subtract, multiply or divide, then enter a value or use a preset. Decimal presets appear when all selected
  fields are floats. The fields change together as one undoable action; invalid or out-of-range
  results leave the whole record unchanged.
- **Copy fields / Paste fields** uses the same selection for every editable leaf type. Copy fields,
  open another record of the same list, then preview and paste values matched by schema path and
  exact runtime type. The record ID is never copied; incompatible, missing and unchanged fields
  are identified before the selected values are applied as one undoable action. Quick edit is
  disabled when the selection also contains non-number fields.
- **Bytes** and the client **Description** below it are collapsible inspector sections. Their
  open states are remembered.
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
**Tools** (Alt+T), beside File and Edit, holds *Export › Selected item… / Selected list…*,
*Import JSON…* and *Translate from elements.data…*. Import and export use JSON only.
A tool panel takes the place of the list and record panes; the shortcuts toggle it, and
the elements.data entry of the bar on the far left goes back to the lists. Tools keep their state
(results, scans) while hidden. After a scan, the status bar shows the problem counts.

The bar on the far left switches between the `elements.data`, `path.data` and `tasks.data` tools.
Other files such as `gshop.data` can get their own workspace later.

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

## Translate from another elements.data

**Tools › Translate from elements.data…** copies human-facing UTF-16 names and text from a
supported translated file into the open file. The versions may differ: lists pair by stable
structure identity, records by a unique ID inside that list, and fields by schema path. For
example, equipment ID 55 in the open v165 list can receive `Iron Sword` from the corresponding
record of a v160 or v165 English file.

The preview shows every list containing compatible text, matched IDs, records and fields ready
to change, missing source IDs and rejected values. Choose the lists to apply and inspect sample
current → translated values first. Blank source strings are not copied. Duplicate IDs,
incompatible fields and strings that do not fit the target field are skipped and reported.
Numeric values, ordinary byte strings and complete record bytes are never transferred. Applying
the selected lists is one undo/history action, and a stale preview cannot be applied after either
file, the open data or the relevant schemas change.

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
| `path` | path.data ID | the client resource path |
| `icon` | path.data ID | the client path and its item-atlas icon |
| `image` | path.data ID | the client path; hover it to preview the standalone package image |
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
