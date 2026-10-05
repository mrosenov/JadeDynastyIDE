import { useCallback, useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Archive, CircleCheck, FileWarning, FolderOpen, Loader2, Save, ShieldCheck, ShieldAlert, ShieldQuestion, TriangleAlert, X } from "lucide-react";
import { saveElements, savePlan } from "../elements/api";
import type { ChecksumCheck, Saved, SaveOptions, SavePlan } from "../elements/types";

interface Props {
  /** Where to save (the open file, or a path picked with Save as). */
  path: string;
  /** path.data chosen earlier in this session. */
  pathData?: string;
  onCancel: () => void;
  onSaved: (saved: Saved, options: SaveOptions) => void;
}

const BACKUP_KEY = "jdide.save.backup";

function readBackup(): boolean {
  try {
    return localStorage.getItem(BACKUP_KEY) !== "0";
  } catch {
    return true;
  }
}

function writeBackup(on: boolean) {
  try {
    localStorage.setItem(BACKUP_KEY, on ? "1" : "0");
  } catch {
    // Remembering is a convenience only.
  }
}

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;
const plural = (n: number, word: string) => `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;

/**
 * Asks before writing elements.data: where to, what changed, the digest the
 * client checks (with which path.data) and whether to keep a backup.
 */
export function SaveDialog({ path: initialPath, pathData: initialPathData, onCancel, onSaved }: Props) {
  const [path, setPath] = useState(initialPath);
  const [pathData, setPathData] = useState<string | undefined>(initialPathData);
  const [backup, setBackup] = useState(readBackup);
  const [plan, setPlan] = useState<SavePlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setPlan(null);
    setError(null);
    savePlan({ path, pathData, backup })
      .then(setPlan)
      .catch((e) => setError(String(e)));
  }, [path, pathData, backup]);

  const run = useCallback(async () => {
    if (saving || !plan) return;
    setSaving(true);
    setError(null);
    // The dialog showed whether the file changed on disk.
    const options: SaveOptions = {
      path,
      pathData,
      backup,
      replaceChanged: true,
    };
    try {
      onSaved(await saveElements(options), options);
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  }, [saving, plan, path, pathData, backup, onSaved]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !saving) onCancel();
      else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
        e.preventDefault();
        run();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel, run, saving]);

  const pickTarget = async () => {
    const picked = await save({
      defaultPath: path,
      filters: [
        { name: "Element data", extensions: ["data"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (picked) setPath(picked);
  };

  const pickPathData = async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      defaultPath: plan?.checksum.pathData,
      filters: [
        { name: "path.data", extensions: ["data"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (typeof picked === "string") setPathData(picked);
  };

  const changes = plan ? plan.changed + plan.added + plan.deleted + plan.dialogs : 0;

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !saving && onCancel()}>
      <div className="modal save-dialog" role="dialog" aria-label="Save elements.data">
        <header className="modal-head">
          <Save size={16} className="accent-icon" />
          <h3 className="truncate">Save {fileName(path)}</h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={onCancel} disabled={saving} aria-label="Close">
            <X size={18} />
          </button>
        </header>

        <div className="save-body">
          <div className="save-target">
            <div className="save-target-path mono truncate" title={path}>
              {path}
            </div>
            <button className="btn small" onClick={pickTarget} disabled={saving} title="Save to another file (Save as)">
              <FolderOpen size={13} /> Save as…
            </button>
          </div>

          {!plan && !error && (
            <div className="muted small">
              <Loader2 size={13} className="spin" /> Checking the file and its checksum…
            </div>
          )}

          {plan && (
            <>
              <div className="save-facts">
                <span className={"tag" + (plan.replaces ? " warn" : "")}>
                  {plan.replaces ? (plan.sameFile ? "Replaces the open file" : "Replaces an existing file") : "New file"}
                </span>
                <span className="muted small">{(plan.size / 1_048_576).toFixed(1)} MB</span>
                <span className="spacer" />
                <span className="save-changes small">
                  {changes === 0 ? (
                    <span className="muted">No changes since the last save</span>
                  ) : (
                    <>
                      {plan.changed > 0 && <span className="chg changed">{plural(plan.changed, "changed record")}</span>}
                      {plan.dialogs > 0 && <span className="chg changed">{plural(plan.dialogs, "translated dialog")}</span>}
                      {plan.added > 0 && <span className="chg added">{plural(plan.added, "added record")}</span>}
                      {plan.deleted > 0 && <span className="chg deleted">{plural(plan.deleted, "deleted record")}</span>}
                    </>
                  )}
                </span>
              </div>

              {plan.changedOnDisk && (
                <div className="save-note danger">
                  <FileWarning size={15} />
                  <div>
                    <b>The file was changed by another program</b> since JD IDE read it. Saving replaces those changes with what is open here.
                  </div>
                </div>
              )}

              {plan.readOnly && (
                <div className="save-note warn">
                  <TriangleAlert size={15} />
                  <div>The file is read-only. Saving clears the read-only flag, as the official editor does.</div>
                </div>
              )}

              <ChecksumNote check={plan.checksum} onPick={pickPathData} disabled={saving} />

              <label className={"save-backup" + (plan.replaces ? "" : " off")}>
                <input type="checkbox" checked={backup} onChange={(e) => (setBackup(e.target.checked), writeBackup(e.target.checked))} disabled={saving || !plan.replaces} />
                <Archive size={14} />
                <span>
                  Keep a backup of the replaced file
                  <span className="muted small save-backup-name">
                    {!plan.replaces ? " · nothing is replaced" : plan.backup ? ` · ${fileName(plan.backup)}` : backup ? " · already backed up in this session" : ""}
                  </span>
                </span>
              </label>
            </>
          )}

          {error && <div className="se-problems">{error}</div>}
        </div>

        <footer className="modal-foot">
          <span className="muted small">Undo keeps working after saving.</span>
          <span className="spacer" />
          <button className="btn" onClick={onCancel} disabled={saving}>
            Cancel
          </button>
          <button className="btn primary" onClick={run} disabled={!plan || saving} autoFocus>
            {saving ? <Loader2 size={14} className="spin" /> : <Save size={14} />} {saving ? "Saving…" : "Save"}
          </button>
        </footer>
      </div>
    </div>
  );
}

/** The digest the client checks on start, and the path.data it is made with. */
function ChecksumNote({ check, onPick, disabled }: { check: ChecksumCheck; onPick: () => void; disabled: boolean }) {
  if (check.status === "no_slots") {
    return (
      <div className="save-note muted">
        <ShieldQuestion size={15} />
        <div>This version of elements.data has no checksum.</div>
      </div>
    );
  }
  const tone = check.status === "valid" || check.status === "not_stored" ? "ok" : "warn";
  const Icon = tone === "ok" ? ShieldCheck : ShieldAlert;
  const message = {
    valid: (
      <>
        <b>Checksum</b> · the file on disk is valid with this path.data. The saved file gets a new checksum for its new contents.
      </>
    ),
    mismatch: (
      <>
        <b>Checksum</b> · the file on disk does not carry the checksum for this path.data. It was likely saved by a tool that leaves the checksum alone (the game client in use then
        does not check it), or this is not the client's path.data. The saved file gets the right checksum for this path.data.
      </>
    ),
    not_stored: (
      <>
        <b>Checksum</b> · the file on disk holds none. The saved file gets one, made with this path.data.
      </>
    ),
    no_path_data: (
      <>
        <b>No path.data found</b> next to the file or in the client folder. The checksum is made from it, so without it the old checksum stays, and a game client that checks it
        refuses the file.
      </>
    ),
  }[check.status];
  return (
    <div className={`save-note ${tone}`}>
      <Icon size={15} />
      <div className="save-note-main">
        <div>{message}</div>
        <div className="save-pathdata">
          {check.pathData ? (
            <>
              <CircleCheck size={12} />
              <span className="mono truncate" title={check.pathData}>
                {check.pathData}
              </span>
              <span className="muted">({check.pathDataFrom})</span>
            </>
          ) : (
            <span className="muted">No path.data chosen</span>
          )}
          <span className="spacer" />
          <button className="btn small" onClick={onPick} disabled={disabled} title="The path.data the game client ships with this elements.data">
            <FolderOpen size={12} /> {check.pathData ? "Change…" : "Choose path.data…"}
          </button>
        </div>
      </div>
    </div>
  );
}
