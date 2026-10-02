import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type {
  FileSummary,
  ImportCandidate,
  ListDef,
  ListSchema,
  RecordDetail,
  RecordRow,
  SchemaContext,
  ClientInfo,
  Settings,
  SettingsView,
  NamedSet,
  SetDetail,
  SetKind,
  SetSummary,
  SetsChanged,
} from "./types";

export const openElements = (path: string) => invoke<FileSummary>("open_elements", { path });

export const listRecords = (list: number) => invoke<RecordRow[]>("list_records", { list });

export const getRecord = (list: number, index: number) => invoke<RecordDetail>("get_record", { list, index });

export const schemaContext = () => invoke<SchemaContext>("schema_context");

export const getListSchema = (list: number) => invoke<ListSchema>("get_list_schema", { list });

export const previewListSchema = (list: number, index: number, def: ListDef) =>
  invoke<RecordDetail>("preview_list_schema", { list, index, def });

export const saveListSchema = (list: number, def: ListDef) => invoke<FileSummary>("save_list_schema", { list, def });

export const resetListSchema = (list: number) => invoke<FileSummary>("reset_list_schema", { list });

export const importCandidates = (list: number) => invoke<ImportCandidate[]>("import_candidates", { list });

export const getSettings = () => invoke<SettingsView>("get_settings");

export const saveSettings = (settings: Settings) => invoke<SettingsView>("save_settings", { settings });

export const inspectClient = (dir: string) => invoke<ClientInfo>("inspect_client", { dir });

/** URL of an item icon served by the app (by path.data ID). No slashes: convertFileSrc encodes them. */
export const iconUrl = (generation: number, pathId: number) => convertFileSrc(`${generation}-${pathId}`, "jdicon");

export const namedSets = () => invoke<SetSummary[]>("named_sets");

export const namedSet = (key: string) => invoke<SetDetail>("named_set", { key });

export const saveNamedSet = (kind: SetKind, set: NamedSet) => invoke<SetsChanged>("save_named_set", { kind, set });

export const deleteNamedSet = (key: string) => invoke<SetsChanged>("delete_named_set", { key });
