import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { CircleAlert, CircleCheck, Database, FolderSearch, Loader2, Package, Settings as SettingsIcon, X } from "lucide-react";
import { inspectClient, saveSettings } from "../elements/api";
import type { ClientInfo, SettingsView } from "../elements/types";
import { bytes } from "../elements/format";

interface Props {
  view: SettingsView;
  onSaved: (view: SettingsView) => void;
  onOpenFile: (path: string) => void;
  onClose: () => void;
}

/** App settings: the game client folder and what was found in it. */
export function SettingsDialog({ view, onSaved, onOpenFile, onClose }: Props) {
  const [dir, setDir] = useState(view.settings.clientDir ?? "");
  const [openOnStart, setOpenOnStart] = useState(view.settings.openOnStart);
  const [client, setClient] = useState<ClientInfo | null>(view.client);
  const [problem, setProblem] = useState<string | null>(view.clientError);
  const [checking, setChecking] = useState(false);
  const [saving, setSaving] = useState(false);

  // Check the folder as it is typed or picked.
  useEffect(() => {
    const trimmed = dir.trim();
    if (!trimmed) {
      setClient(null);
      setProblem(null);
      return;
    }
    let cancelled = false;
    setChecking(true);
    const timer = setTimeout(() => {
      inspectClient(trimmed)
        .then((info) => {
          if (cancelled) return;
          setClient(info);
          setProblem(null);
        })
        .catch((e) => {
          if (cancelled) return;
          setClient(null);
          setProblem(String(e));
        })
        .finally(() => !cancelled && setChecking(false));
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [dir]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const browse = async () => {
    const picked = await open({ directory: true, multiple: false, title: "Pick the game client folder" });
    if (typeof picked === "string") setDir(picked);
  };

  const save = async () => {
    setSaving(true);
    try {
      onSaved(await saveSettings({ clientDir: dir.trim() || null, openOnStart }));
      onClose();
    } catch (e) {
      setProblem(String(e));
    } finally {
      setSaving(false);
    }
  };

  const changed = (view.settings.clientDir ?? "") !== dir.trim() || view.settings.openOnStart !== openOnStart;
  const canSave = !saving && !checking && (!dir.trim() || client !== null);

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal settings-dialog" role="dialog" aria-label="Settings">
        <header className="modal-head">
          <SettingsIcon size={17} />
          <h3>Settings</h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </header>

        <div className="modal-body scroll">
          <section className="settings-section">
            <h4>Game client</h4>
            <p className="muted small">
              The client folder holds the data files (element\data) and the resource packages (element\*.pck). JD IDE
              reads path.data and the item icons from it.
            </p>
            <div className="settings-row">
              <input
                className="se-input mono"
                value={dir}
                placeholder="e.g. E:\Games\ForsakenJD"
                onChange={(e) => setDir(e.target.value)}
                spellCheck={false}
              />
              <button className="btn" onClick={browse}>
                <FolderSearch size={15} /> Browse…
              </button>
              {dir && (
                <button className="btn" onClick={() => setDir("")} title="Forget the client folder">
                  Clear
                </button>
              )}
            </div>

            {checking && (
              <div className="settings-status muted">
                <Loader2 size={14} className="spin" /> Checking the folder…
              </div>
            )}
            {!checking && problem && (
              <div className="settings-status bad">
                <CircleAlert size={14} /> {problem}
              </div>
            )}
            {!checking && client && (
              <>
                <div className="settings-status ok">
                  <CircleCheck size={14} />
                  Client found: {client.dataFiles.length} data files, {client.packages.length} packages
                  {client.hasPathData ? ", path.data" : ""}
                  {client.hasItemIcons ? ", item icons" : ""}
                </div>
                <label className="check settings-check">
                  <input
                    type="checkbox"
                    checked={openOnStart}
                    onChange={(e) => setOpenOnStart(e.target.checked)}
                    disabled={!client.elementsPath}
                  />
                  Open the client's elements.data when JD IDE starts
                </label>

                <h5>
                  <Database size={14} /> Data files <span className="muted">{client.elementDir}\data</span>
                </h5>
                <div className="settings-table">
                  {client.dataFiles.map((f) => (
                    <div className="settings-file" key={f.path}>
                      <span className="mono truncate">{f.name}</span>
                      <span className="muted small">{f.kind}</span>
                      <span className="muted small mono">{bytes(f.size)}</span>
                      {f.supported ? (
                        <button
                          className="link"
                          onClick={() => {
                            onOpenFile(f.path);
                            onClose();
                          }}
                        >
                          Open
                        </button>
                      ) : (
                        <span className="size-badge differs" title="Reading this kind of file is planned for a later version">
                          later
                        </span>
                      )}
                    </div>
                  ))}
                </div>

                <h5>
                  <Package size={14} /> Packages <span className="muted">{client.elementDir}</span>
                </h5>
                <div className="settings-table">
                  {client.packages.map((p) => (
                    <div className="settings-file" key={p.path}>
                      <span className="mono truncate">{p.name}</span>
                      <span className="muted small">{p.parts > 1 ? `${p.parts} parts` : ""}</span>
                      <span className="muted small mono">{bytes(p.size)}</span>
                      <span />
                    </div>
                  ))}
                </div>
              </>
            )}
          </section>
        </div>

        <footer className="modal-foot">
          <span className="muted small">Settings are saved in the app's config folder.</span>
          <span className="spacer" />
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" onClick={save} disabled={!canSave || !changed}>
            {saving ? "Saving…" : "Save"}
          </button>
        </footer>
      </div>
    </div>
  );
}
