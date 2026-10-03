import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Check, Copy, Pencil, X } from "lucide-react";
import { namedSet } from "../elements/api";
import type { FieldNode, SetDetail } from "../elements/types";
import { bitHex, bitValue, bitWidth, hasBit, hex, toBits, toSigned } from "../elements/bits";

interface Props {
  node: FieldNode;
  anchor: DOMRect;
  onEdit: (key: string) => void;
  onClose: () => void;
  /** Sets the field to a value (the calculator's, or an enum value clicked). */
  onApply?: (value: string) => Promise<string | null>;
}

/**
 * A field's enum or mask at a glance. For masks it is a calculator: tick the
 * bits to see (and copy) the resulting value.
 */
export function SetPopover({ node, anchor, onEdit, onClose, onApply }: Props) {
  const [detail, setDetail] = useState<SetDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const width = bitWidth(node.ty);
  const original = toBits(node.value ?? "0", width);
  const [bits, setBits] = useState(original);
  const [copied, setCopied] = useState<string | null>(null);
  const [applyError, setApplyError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const apply = async (value: string) => {
    if (!onApply || applying) return;
    setApplying(true);
    setApplyError(null);
    const problem = await onApply(value);
    setApplying(false);
    if (problem) setApplyError(problem.replace(/^Error: /, ""));
    else onClose();
  };
  const box = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: anchor.left, top: anchor.bottom + 6 });
  // Once dragged, the popover stays where the user put it.
  const moved = useRef(false);
  const drag = useRef<{ dx: number; dy: number } | null>(null);

  const startDrag = (e: React.PointerEvent) => {
    if ((e.target as HTMLElement).closest("button") || e.button !== 0) return;
    drag.current = { dx: e.clientX - pos.left, dy: e.clientY - pos.top };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  };
  const onDrag = (e: React.PointerEvent) => {
    if (!drag.current || !box.current) return;
    const { width: w, height: h } = box.current.getBoundingClientRect();
    moved.current = true;
    setPos({
      left: Math.max(0, Math.min(e.clientX - drag.current.dx, window.innerWidth - w)),
      top: Math.max(0, Math.min(e.clientY - drag.current.dy, window.innerHeight - h)),
    });
  };
  const endDrag = () => {
    drag.current = null;
  };

  useEffect(() => {
    if (!node.set) return;
    namedSet(node.set)
      .then(setDetail)
      .catch((e) => setError(String(e)));
  }, [node.set]);

  // Keep the popover on screen.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el || moved.current) return;
    const { width: w, height: h } = el.getBoundingClientRect();
    const left = Math.max(8, Math.min(anchor.left, window.innerWidth - w - 8));
    const below = anchor.bottom + 6;
    const top = below + h > window.innerHeight - 8 ? Math.max(8, anchor.top - h - 6) : below;
    setPos({ left, top });
  }, [anchor, detail]);

  useEffect(() => {
    const onDown = (e: MouseEvent) => !box.current?.contains(e.target as Node) && onClose();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  const copy = (text: string, what: string) => {
    navigator.clipboard.writeText(text);
    setCopied(what);
    setTimeout(() => setCopied(null), 1200);
  };

  const isMask = detail?.kind === "mask";
  const flags = detail?.set.flags ?? [];
  // Bits set in the value that the mask does not name stay toggleable.
  const unnamed = isMask
    ? Array.from({ length: width }, (_, b) => b).filter((b) => hasBit(original | bits, b) && !flags.some((f) => f.bit === b))
    : [];
  const signedType = !node.ty.startsWith("u");
  // Characters of the widest hex value shown: "0x" and one digit per 4 bits.
  const topBit = Math.max(0, ...flags.map((f) => f.bit), ...unnamed);
  // (plus room for the gap: the column is measured in the label font, not the code font).
  const hexChars = isMask ? Math.max(8, 4 + Math.ceil((topBit + 1) / 4)) : 8;
  const decimal = (signedType ? toSigned(bits, width) : bits).toString();

  return (
    <div
      className="set-popover"
      ref={box}
      // The hex column fits the widest bit value (0x8000000000000000 for 64-bit masks).
      style={{ ...pos, ["--hex-w" as string]: `${hexChars}ch`, width: hexChars > 12 ? 420 : 340 }}
      role="dialog"
      aria-label={`${node.name} values`}
    >
      <div
        className="set-popover-head"
        onPointerDown={startDrag}
        onPointerMove={onDrag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        title="Drag to move"
      >
        <span className="truncate">
          <b>{node.name}</b> <span className="muted">· {detail?.set.label ?? node.set}</span>
        </span>
        <span className="spacer" />
        {node.set && (
          <button className="icon-btn small" onClick={() => onEdit(node.set!)} title={`Edit this ${isMask ? "mask" : "enum"}`}>
            <Pencil size={13} />
          </button>
        )}
        <button className="icon-btn small" onClick={onClose} aria-label="Close">
          <X size={15} />
        </button>
      </div>
      {error && <div className="empty-note">{error}</div>}
      {!detail && !error && <div className="empty-note">Loading…</div>}

      {detail && isMask && (
        <>
          <div className="set-popover-list scroll">
            {[...flags.map((f) => ({ bit: f.bit, label: f.label, description: f.description })), ...unnamed.map((b) => ({ bit: b, label: `bit ${b} (unnamed)`, description: "" }))]
              .sort((a, b) => a.bit - b.bit)
              .map((f) => (
                <label
                  key={f.bit}
                  className={"mask-bit" + (hasBit(bits, f.bit) ? " on" : "") + (hasBit(bits, f.bit) !== hasBit(original, f.bit) ? " changed" : "")}
                  title={f.description || undefined}
                >
                  <input type="checkbox" checked={hasBit(bits, f.bit)} onChange={() => setBits((v) => v ^ bitValue(f.bit))} />
                  <span className="mono muted small">{bitHex(f.bit)}</span>
                  <span className="truncate">{f.label}</span>
                </label>
              ))}
          </div>
          <div className="set-popover-foot">
            <div className="mask-result">
              <span className="muted small">Value</span>
              <button className="link mono" onClick={() => copy(decimal, "dec")} title="Copy">
                {decimal} {copied === "dec" ? <Check size={12} /> : <Copy size={12} />}
              </button>
              <button className="link mono" onClick={() => copy(hex(bits, width), "hex")} title="Copy">
                {hex(bits, width)} {copied === "hex" ? <Check size={12} /> : <Copy size={12} />}
              </button>
            </div>
            {bits !== original && (
              <div className="mask-actions">
                <button className="link" onClick={() => setBits(original)}>
                  Reset to the record's value
                </button>
                {onApply && (
                  <button className="btn primary small" onClick={() => apply(decimal)} disabled={applying} title="Set the field to this value (undo with Ctrl+Z)">
                    <Check size={13} /> Apply {decimal}
                  </button>
                )}
              </div>
            )}
            {applyError && <div className="se-problems">{applyError}</div>}
          </div>
        </>
      )}

      {detail && !isMask && (
        <div className="set-popover-list scroll">
          {(detail.set.values ?? []).map((v) =>
            onApply ? (
              <button
                key={v.value}
                className={"enum-value pick" + (String(v.value) === node.value ? " on" : "")}
                title={(v.description ? `${v.description}\n` : "") + "Click to set the field to this value"}
                onClick={() => String(v.value) !== node.value && apply(String(v.value))}
                disabled={applying}
              >
                <span className="mono muted small">{v.value}</span>
                <span className="truncate">{v.label}</span>
              </button>
            ) : (
              <div key={v.value} className={"enum-value" + (String(v.value) === node.value ? " on" : "")} title={v.description || undefined}>
                <span className="mono muted small">{v.value}</span>
                <span className="truncate">{v.label}</span>
              </div>
            ),
          )}
          {applyError && <div className="se-problems">{applyError}</div>}
          {!(detail.set.values ?? []).some((v) => String(v.value) === node.value) && (
            <div className="enum-value on">
              <span className="mono muted small">{node.value}</span>
              <span className="muted">not in this enum</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
