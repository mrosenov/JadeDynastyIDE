# tasks.data editor build plan

This plan covers the static task editor stored as `tasks.data` plus its numbered companion packs. `dyn_tasks.data` and `task_npc.data` are related formats, but they are separate follow-up tools.

## Goals

- Open, browse, search, inspect, edit and safely save static tasks.
- Support task formats v165, v172 and v184 first.
- Preserve fields whose purpose is unknown as exact typed or opaque values.
- Make future task versions extendable through version patches instead of another full parser rewrite.
- Never enable saving for a task format until the complete file set passes structural and byte-exact verification.

## Verified fixtures

Tests may read these files but must only write to temporary directories.

| Fixture | Task version | Root tasks | Packs | Purpose |
|---|---:|---:|---:|---|
| `E:/Games/XtremeJade/element/data/tasks.data` | 165 | 13,450 | 45 | Exact matching C++ source and first implementation target |
| `E:/Games/ForsakenJD/element/data/tasks.data` | 172 | 15,211 | 51 | Main v172 client fixture; Jade Editor reads and saves it |
| `E:/Games/Elite Jade Dynasty - HDN/element/data/tasks.data` | 184 | 17,882 | 60 | Main v184 client fixture |
| `E:/Game Dev/JD/1792/gamed/config/tasks.data` | 184 | 17,860 | 60 | Independent v184 server fixture |

The XtremeJade v165 index is 740 bytes (`20 + 45 * 16`), all 45 packs are present, and the stored MD5 for pack 1 matches the actual file.

## Format sources

- v165 field meaning and serialization: `E:/Game Dev/ZX-Client-Src/ZElement/ZElementClient/Task/TaskTempl.h`, `TaskTempl.cpp` and `TaskTemplMan.cpp`.
- Version-aware read/write behavior, including v172 and v184: `E:/Game Dev/JD/Tools/Jade Editor/Jade Editor.dll`.
- Real files are the final authority. A format is supported for saving only when it round-trips those files exactly.

## Safety rules

1. Sample files are always opened read-only.
2. Corrupt headers, missing packs, invalid offsets, MD5 mismatches and unreasonable counts are reported before task parsing begins.
3. Parsing has explicit byte, array, recursion and allocation limits.
4. Unknown fixed-width values default to raw types such as `raw8`, `raw16`, `raw32` or `bytes[N]`.
5. A no-edit save must reproduce every original pack byte-for-byte.
6. Saving writes a complete temporary file set, verifies it by reopening it, then replaces the target files.
7. Structural operations stay narrow and independently verified. Top-level and subtree cloning,
   subtree and top-level deletion, and subtree reparenting are implemented; explicit sibling
   reordering remains disabled.

## Milestone 1: container reader

**Status: complete (2026-10-06).** The reader validates the index, every numbered pack, MD5 digests and root-offset tables, then exposes complete root trees on demand. It passes the synthetic corruption tests and all four real fixtures above.

Implement the task index and numbered-pack layer without decoding task records.

- Read and validate `TASK_PACK_INFO`.
- Discover `tasks.data1` through `tasks.dataN`.
- Verify every stored MD5.
- Read each pack header and root offset table.
- Validate sorted, in-range offsets and exact pack boundaries.
- Expose root records lazily so opening a 200+ MiB task set does not decode everything immediately.

Completion gate:

- All four fixtures report their expected version, root count and pack count.
- Truncated, missing, reordered and checksum-mismatched test copies fail with precise errors.

## Milestone 2: versioned task schema engine

**Status: complete (2026-10-06).** The declarative engine supports bounded byte-preserving decode/encode, source offsets, numeric and raw types, fixed and prefixed UTF-16 text, reusable structs, fixed and counted arrays, conditions, recursive children, and ordered version patches.

Build a declarative binary layout engine before adding individual task versions. The engine must support:

- Primitive integers, floats, one-byte booleans and fixed byte blocks.
- Fixed UTF-16LE text and length-prefixed UTF-16LE text.
- Reusable structures and fixed arrays.
- Arrays whose length is read from another field.
- Conditions based on task version, flags, enum values or counts.
- Recursive child tasks.
- Raw fields that preserve bytes while offering several display interpretations.
- A base version plus ordered patches such as “insert `raw32` after field X since v203”.

Keep binary shape separate from labels, descriptions, roles and reference metadata. Improving an unknown field's name must not change serialization.

Completion gate:

- Synthetic schemas cover fixed fields, conditional sections, counted arrays, nested structures and recursion.
- Every parsed node records its source offset and byte length.

## Milestone 3: shared task structures

**Status: complete for the v165 base (2026-10-06).** Packed shared records, task text, dialog trees, expressions, candidate rewards, complete reward headers and scaled rewards now decode and encode symmetrically. Later-version additions remain version patches in Milestone 5.

Implement and test the structures used by task records:

- Time and coordinate structures.
- Required and awarded items.
- Monster, player, region and team requirements.
- Expressions and change-key data.
- Success and failure awards, including scaled and candidate rewards.
- Task dialogs: delivery, unqualified, item delivery, execution and award.
- Recursive task hierarchy.

Completion gate:

- Each structure has symmetric decode and encode tests.
- Invalid counts and truncated structures fail without reading beyond their record.

## Milestone 4: task format v165

**Status: complete (2026-10-06).** The v165 schema maps the 2,490-byte packed header and every variable section: signatures, timetables, change-key arrays, requirements, compare expressions, rewards, text, dialogs and recursive children. Every one of XtremeJade's 13,450 root trees parses to its indexed boundary and re-encodes to its exact original bytes.

Describe v165 from the matching C++ source and verify it against XtremeJade.

Completion gate:

- Every root and nested task in all 45 packs parses successfully.
- Each root ends at its indexed boundary.
- Re-encoding every root produces its exact original bytes.
- Rebuilding all packs produces their exact original bytes and MD5 values.

## Milestone 5: task formats v172 and v184

**Status: complete (2026-10-07).** v172 passes all 15,211 ForsakenJD roots. v184 independently passes all 17,882 HDN roots and all 17,860 roots in the 1792 server fixture. Every decoded root, including nested tasks, re-encodes to its exact original bytes.

The v172 fixed header is 2,555 bytes: it adds a one-byte `kermis` flag and expands the zone-friendship array by 16 entries. Its rewards make the same friendship change and add star-soul values. The v184 fixed header is 2,571 bytes and adds four still-unknown `u32` values. Its reward header adds byte-preserved unknown fields and an additional candidate collection. Dialog windows can include two integer parameters and counted UTF-16 parameter text. Five v174 values are stored after the dialog bundle. Unknown names remain deliberately generic while their binary types and positions are proven.

Add version patches from Jade Editor's symmetric reader/writer behavior.

- v172 must pass ForsakenJD.
- v184 must independently pass both HDN and the 1792 server fixture.
- Fields with unknown meaning retain stable names such as `unknown_v184_001` and preserve their exact representation.

Completion gate for each version:

- Zero parse failures across every pack and nested task.
- Exact root, pack and index round trips.
- Matching results on independent fixtures where available.

## Milestone 6: read-only task browser

**Status: complete (2026-10-07).** The browser is wired into the activity bar. It auto-opens the
configured client's task index or accepts another file, reports verified integrity and file
statistics, searches and pages root and nested tasks, expands the recursive hierarchy inline,
and lazily decodes one cached root into a categorized, collapsible typed inspector with offsets,
raw-value interpretations and links for task, elements and client-resource references. Root
summaries remain fast and nested search indexing completes in background work.

Add a Tasks activity-bar tool that loads the task set beside the selected client or lets the user choose another index.

- Tree view for root tasks and subtasks.
- Search by ID and name.
- Task-version, root-count, pack-count and integrity summary.
- Categorized inspector: general, availability, prerequisites, objectives, failure, rewards, dialogs and hierarchy.
- Raw/hex view for unknown fields.
- Links to matching elements records for items, NPCs, monsters, skills, buffs and titles where the meaning is known.
- Load packs and expensive sections on demand; keep search indexing in background work.

Completion gate:

- The UI remains responsive with the HDN v184 fixture.
- Selecting, expanding and searching tasks does not decode unrelated packs repeatedly.

## Milestone 7: editing model

**Status: complete (2026-10-07).** Safe leaf edits now stay in memory as complete replacement
root bytes. Scalars, fixed and variable text, existing array/dialog leaves and exact-width raw
values are editable; structure-driving counts, condition fields and task IDs remain locked.
Variable text keeps its count synchronized, and every result must decode and re-encode exactly.
Changed roots/fields, task-specific history, undo/redo and revert-all are wired into the UI.

Add changes in memory before implementing disk saving.

- Edit known scalar values, names and text.
- Edit existing array entries and dialog text.
- Show raw values using hex plus optional integer/float interpretations.
- Undo, redo, history, changed markers and revert-all behavior consistent with the elements editor.
- Validate ranges, counts, references and structural conditions before accepting an edit.

Unknown values remain editable only through a representation that preserves their declared byte width.

Completion gate:

- Editing and undoing restores the original bytes exactly.
- Changing variable-length text correctly updates the containing root record in memory.

## Milestone 8: safe saving

**Implementation complete and validated (October 2026).**

The saver began with existing-task edits and now also supports appending cloned top-level roots to
an existing pack with capacity.

- Rewrite only changed packs when possible.
- Recalculate root offsets inside every changed pack.
- Recalculate changed-pack MD5 entries in `tasks.data`.
- Preserve unchanged packs byte-for-byte.
- Use backup, temporary-write, atomic replacement and changed-on-disk protection comparable to `elements.data` saving.
- Reopen and fully validate the temporary output before replacing the target.

Completion gate:

- No-edit save is byte-identical.
- Controlled edits change only the expected root, its pack offsets when necessary, and that pack's MD5 entry.
- The user confirms a saved copy loads in the matching client/server.

The final gate passed on October 8, 2026: a real v165 top-level task was cloned and saved, and
both the matching server and client started successfully without a crash.

## Milestone 9: unsupported-version analyzer and schema editor

Update (October 2026): the analyzer first probes every built-in layout on a sample and offers one that
reads it exactly, and **Align with a supported task set** (`tasks/align.rs`) proposes a complete user
patch (new raw `unknown_v<ver>_N` fields, removals, resizes, unknown blocks) with evidence and an
unresolved list. User layouts are frozen at their base version, so the base may be newer than the file.

**Complete (October 2026).** Unsupported files now open in a read-only analyzer with selectable
v165/v172/v184 baselines, parallel whole-file root and byte coverage, first-failure offsets and
per-pack results. A second supported task set can be compared by root ID, with added/removed/renamed
IDs and recurring root-size deltas summarized as structural evidence. The analyzer now scores
1/2/4/8/16/32-byte insertions at known field boundaries across matching changed roots, including
alignment confidence, type hints and example bytes. A user can accept one suggestion as a named,
version-gated fixed-width insertion, keep the safe raw default or choose a same-width integer, float,
boolean or byte type, persist it under `task-layouts/v<version>.json`, change its type, remove it later,
and re-run complete coverage after every operation. Candidate scoring is patch-aware: it normalizes
accepted fixed-width spans in temporary root copies and can anchor the next adjacent field after an
earlier accepted field. Complete versioned patches can be exported and imported as JSON; imports
must match the open task version and pass schema validation plus whole-file coverage before replacing
the active patch. The task header now opens an explicit searchable schema browser for supported and
unsupported files. It presents verified built-ins as read-only and shows the effective baseline plus
accepted user fields for an unsupported version. Inserted fields can now have multiple validated
controller-field predicates; changing them reruns whole-file coverage, and iterative scoring applies
them per record. A guarded manual operation adds counted arrays of scalar values or existing task
structures, requiring an earlier integer count field and rerunning whole-file coverage before it is
stored. Inherited baseline fields can now be removed or assigned another fixed-width type, with
dependency validation and whole-file analysis. A candidate layout can be promoted only after every
root decodes and re-encodes byte-for-byte and the normal task browser also reads the complete set.
Promotion stores a digest of the exact schema operations, opens that version in the editable task
workspace, and is cleared automatically by any later schema change. Accepted layouts can be sent
back to the analyzer explicitly for further work. An unchanged older baseline may also be promoted
when it already matches the newer version exactly.

Build the workflow for future task versions after the known parsers and saver are trusted.

- Start from the closest supported schema.
- Show the first field or section where parsing loses alignment.
- Compare matching task IDs across old and new task sets.
- Try candidate fixed-width raw fields and score them across many records.
- Allow version-gated insertions, removals, type changes, conditions and counted arrays.
- Display parse coverage by roots, nested tasks and packs.
- Preserve unresolved regions as opaque bytes when their fixed length is known.
- Export/import user task-layout patches.

Saving remains unavailable until the candidate schema parses every record, passes an exact no-edit
round trip and is explicitly accepted.

### Paused: v186 (Jade Dynasty Reborn) alignment — status October 9, 2026

Work stopped here at the user's request; nothing is committed yet. Files: `tasks/align.rs` (new),
`tasks/analyze.rs` (`probe_layouts`), `lib.rs` (`probe_task_layouts`, `propose_task_alignment`,
`apply_task_alignment`), `TasksEditor.tsx` (`LayoutProbes`, `AlignmentSection`), `api.ts`, `types.ts`,
`App.css`. All 167 Rust tests, `tsc` and the Vite build passed at the pause.

Fixtures: target `E:/Games/Jade Dynasty Reborn/element/data/tasks.data` (v186, 18,160 roots),
reference `E:/Games/Elite Jade Dynasty - HDN/element/data/tasks.data` (v184). The user's friend has a
v186 tool but cannot share its layout; they say v186 is a pre-release build from before the x64 client.

What the data shows (v186 vs v184):

- Every single-task root is exactly 679 bytes smaller (10,284 of 17,882 paired roots); roots with
  subtasks are smaller by exact multiples. The change is purely in the fixed layout.
- Header (`TASK_FIXED_V184`): about −387 bytes. Nearly all `*_pointer` fields and vector
  pointers/capacities are absent; `have_fail_items` 16 → 4, `life_again_*_occupation` 45 → 1,
  `title_wanted` shorter. The four `COMPARE_EXPRESSION` blocks (finish/premise compares) lose their
  vector pointers too (solved by flattening).
- Each `AWARD_DATA`: −144 measured by hand (quest 30): −68 before `transform_id`, −84 between
  `transform_id` and `candidates`, **+8 after the candidate list**. The solver gets −148 with no tail
  insertion, because its anchor (`experience_coefficient_2`) sits among three equal 1.0 coefficients
  and can match one field off (4 bytes); `experience` (u64) does not match in v186 either, so the
  award's first fields probably changed too.
- The task record gets a compensating `+4 after change_types` from the event fallback.
- Texts, dialogs, scaled awards and timetables are unchanged.

Result at the pause: 406/1,111 sampled quests exact, about 30% of all roots
(`finds_shorter_lists_and_missing_pointers_in_v186`). About 12,000 of the failures stop at
`success_award.extra_tribute`.

Next steps when resuming:

1. Settle the award block: pick quests whose award tail (after `transform_id`, and after
   `candidates`) holds non-zero values, and dump both files side by side (the probe pattern in
   AGENTS.md). Find the 8 extra bytes after `candidates` and the true head change (gold/experience).
2. Then either fix the anchor ambiguity in `solve_flat` (an anchor from a later distinctive field,
   or the backward pass reaching the start) or add the confirmed facts as manual patch operations.
3. Rerun the decode statistics over all roots (decode every root with the patched schema and group
   `decode_prefix_diagnostic` failures by field), aiming for 100% before accepting the layout.

## Later work

- Clone, delete, move and reparent tasks. Top-level task cloning plus subquest subtree cloning,
  deletion and moving are now available as undoable operations. Clones receive fresh IDs and
  internal-reference remapping; a top-level clone appends to an existing pack with capacity and
  rebuilds its index data when saved. Deletion previews surviving references and requires explicit
  confirmation if any become unresolved; moving preserves IDs and updates both parent counts,
  including across roots and packs. Top-level deletion (October 2026) keeps a tombstone until save,
  then rebuilds the pack. Explicit sibling ordering remains later work.
- ~~Compare and transfer compatible fields between task files.~~ Done (October 2026): Compare panel
  pairing tasks by ID across versions, lazy field diffs, patch notes, and copying fields or whole
  top-level trees through the JSON import checks.
- ~~Translation workflow for names, descriptions and dialog text by task ID.~~ Done (October 2026):
  Tools › Translate from tasks.data with Names, Descriptions and Dialogs groups, shape-checked talks
  and the open file's terminator convention.
- ~~JSON import/export.~~ Done (October 2026): versioned `jdide-tasks` export of a task, a tree or
  the listed tasks; import previews and applies field updates by task ID with the inspector's
  checks and adds missing top-level trees from `_raw` when version and layout match.
- ~~Task problems scanner and reference graph.~~ Done (October 2026): Problems panel from the
  background index (duplicate and skipped IDs, broken and self references, stale hierarchy links,
  missing elements.data items/monsters, full packs) and the inspector's Referenced by section.
  Premise and mutex task lists are now named fields in every supported layout.
- A separate editor for `task_npc.data`. (The `dyn_tasks.data` editor exists: its own workspace, see README.)
- Optional developer-only loader tracing for versions that cannot be resolved by schema comparison.

## Recommended implementation order

Milestones 1–9, the problems scanner, JSON export/import, compare/transfer and translation are
complete, as are the dialog editor, quest ID changes and top-level deletion. Remaining:
`dyn_tasks.data` and `task_npc.data` editors, creating a numbered pack when every pack is full, and
optional loader tracing.
