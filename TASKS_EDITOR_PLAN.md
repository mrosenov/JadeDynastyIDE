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
7. The first release edits existing tasks only. Clone, delete, reparent and reorder remain disabled until pack rebuilding is proven separately.

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

**Implementation complete (October 2026); matching client/server load confirmation pending.**

Initially save existing-task edits without changing root ordering or pack membership.

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

## Milestone 9: unsupported-version analyzer and schema editor

Build the workflow for future task versions after the known parsers and saver are trusted.

- Start from the closest supported schema.
- Show the first field or section where parsing loses alignment.
- Compare matching task IDs across old and new task sets.
- Try candidate fixed-width raw fields and score them across many records.
- Allow version-gated insertions, removals, type changes, conditions and counted arrays.
- Display parse coverage by roots, nested tasks and packs.
- Preserve unresolved regions as opaque bytes when their fixed length is known.
- Export/import user task-layout patches.

Saving remains unavailable until the candidate schema parses every record and passes an exact no-edit round trip.

## Later work

- Clone, delete, move and reparent tasks.
- Compare and transfer compatible fields between task files.
- Translation workflow for names, descriptions and dialog text by task ID.
- JSON import/export.
- Task problems scanner and reference graph.
- Separate editors for `dyn_tasks.data` and `task_npc.data`.
- Optional developer-only loader tracing for versions that cannot be resolved by schema comparison.

## Recommended implementation order

Complete milestones 1–5 in the Rust backend before building the main UI. Then deliver the read-only browser, followed by in-memory editing and finally saving. The unsupported-version schema editor comes after known-version saving because its validation depends on a proven parser and serializer.
