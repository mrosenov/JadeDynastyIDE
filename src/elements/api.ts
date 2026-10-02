import { invoke } from "@tauri-apps/api/core";
import type {
  FileSummary,
  ImportCandidate,
  ListDef,
  ListSchema,
  RecordDetail,
  RecordRow,
  SchemaContext,
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
