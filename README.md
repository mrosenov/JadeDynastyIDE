# JD IDE

Desktop editor for Jade Dynasty (Zhu Xian) data files, built with Tauri 2
(Rust backend, React + TypeScript UI).

Milestone 1: read-only browsing of `elements.data`.

- Opens any version (tested with v66, v112, v156, v158, v160 and v165). The reader
  identifies every segment from its content (lists, checksum slots, exporter
  tags, the NPC dialog block), so newer versions with extra lists still parse.
- Shows list names and typed record fields where a **profile** is known:
  - **v156**: names and full layouts for all 193 lists, generated from the server
    template sources (`exptypes.h` / `elementdataman.cpp`), including the
    original field comments.
  - **v112**: Jade Editor list names, plus typed layouts for AddedAttribute,
    Equipment, Material and NormalItem.
  - Other versions with the same list grouping (e.g. v158–v165) borrow the v156
    names by position, marked in italics. A layout is used when the record size
    matches, or as a partial layout when records have grown.
- The inspector links the field tree and the hex view: hover or click a field to
  highlight its bytes, or click a byte to jump to its field.

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

Rust tests parse real files from `E:/Game Dev/JD` (override with the
`JDIDE_SAMPLES` environment variable). They skip any sample they cannot find.

```powershell
cd src-tauri
cargo test
```

## Profiles

Profiles live in `src-tauri/profiles/*.json` and are embedded into the binary.

Regenerate v156 from the server sources. The optional third argument validates
every computed struct size against a real file:

```powershell
node tools/gen-elements-schema.mjs "E:/Game Dev/JD/zx_source/zgame/gs/template" src-tauri/profiles/v156.json "E:/Game Dev/JD/zxserver/zgame/gs/config/elements.data"
```

Regenerate v112 from the JadeEditorPython format files:

```powershell
node tools/import-jade-editor-profile.mjs "E:/Game Dev/JD/Tools/JadeEditorFOX/JadeEditorPython/formats/elements" src-tauri/profiles/v112.json
```

To support another version exactly, point the generator at that version's
template sources and add the output to `PROFILE_SOURCES` in
`src-tauri/src/elements/profile.rs`.
