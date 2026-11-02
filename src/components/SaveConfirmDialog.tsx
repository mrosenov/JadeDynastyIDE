import { useEffect, type ReactNode } from "react";
import { Archive, FileWarning, Loader2, Save, X } from "lucide-react";
import { useI18n } from "../i18n";

interface Props {
  fileName: string;
  path: string;
  busy: boolean;
  changedOnDisk: boolean;
  backup: boolean;
  onBackupChange: (backup: boolean) => void;
  onCancel: () => void;
  onConfirm: () => void;
  facts?: ReactNode;
  children?: ReactNode;
  footerNote?: ReactNode;
}

/** The shared save confirmation used by format-specific editors. */
export function SaveConfirmDialog({ fileName, path, busy, changedOnDisk, backup, onBackupChange, onCancel, onConfirm, facts, children, footerNote }: Props) {
  const { t } = useI18n();
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) {
        event.preventDefault();
        event.stopPropagation();
        onCancel();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy, onCancel]);
  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onCancel()}>
      <div className="modal save-dialog" role="dialog" aria-modal="true" aria-label={t("save.title", { file: fileName })}>
        <header className="modal-head">
          <Save size={16} className="accent-icon" />
          <h3 className="truncate">{t("save.title", { file: fileName })}</h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={onCancel} disabled={busy} aria-label={t("common.close")}><X size={18} /></button>
        </header>
        <div className="save-body">
          <div className="save-target"><div className="save-target-path mono truncate" title={path}>{path}</div></div>
          {facts && <div className="save-facts">{facts}</div>}
          {changedOnDisk && <div className="save-note danger"><FileWarning size={15} /><div><b>{t("save.changedOnDiskTitle")}</b> {t("save.changedOnDiskBody")}</div></div>}
          {children}
          <label className="save-backup">
            <input type="checkbox" checked={backup} disabled={busy} onChange={(event) => onBackupChange(event.target.checked)} />
            <Archive size={14} />
            <span>{t("save.keepBackup")}<span className="muted small save-backup-name"> · {t("save.backupLocation")}</span></span>
          </label>
        </div>
        <footer className="modal-foot">
          <span className="muted small">{footerNote ?? t("save.undoAvailable")}</span>
          <span className="spacer" />
          <button className="btn" onClick={onCancel} disabled={busy}>{t("common.cancel")}</button>
          <button className="btn primary" onClick={onConfirm} disabled={busy} autoFocus>
            {busy ? <Loader2 size={14} className="spin" /> : <Save size={14} />} {busy ? t("common.saving") : changedOnDisk ? t("common.replaceAnyway") : t("common.save")}
          </button>
        </footer>
      </div>
    </div>
  );
}
