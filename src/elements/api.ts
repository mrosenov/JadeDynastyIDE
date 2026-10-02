import { invoke } from "@tauri-apps/api/core";
import type { FileSummary, RecordDetail, RecordRow } from "./types";

export const openElements = (path: string) => invoke<FileSummary>("open_elements", { path });

export const listRecords = (list: number) => invoke<RecordRow[]>("list_records", { list });

export const getRecord = (list: number, index: number) => invoke<RecordDetail>("get_record", { list, index });
