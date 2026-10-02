import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

interface Props {
  bytes: number[];
  fileOffset: number;
  highlight: { off: number; size: number } | null;
  /** Scroll the highlighted range into view when this changes. */
  focusKey: string | null;
  onPick: (offset: number) => void;
}

const PER_ROW = 16;
const ROW_HEIGHT = 20;

const printable = (b: number) => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : "·");

export function HexView({ bytes, fileOffset, highlight, focusKey, onPick }: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const rows = Math.ceil(bytes.length / PER_ROW);
  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  });

  useEffect(() => {
    if (highlight && focusKey) virtualizer.scrollToIndex(Math.floor(highlight.off / PER_ROW), { align: "auto" });
    // Only re-run when the focused field changes, not on every hover.
  }, [focusKey]);

  const inRange = (i: number) => highlight !== null && i >= highlight.off && i < highlight.off + highlight.size;
  const digits = Math.max(4, (bytes.length - 1).toString(16).length);

  return (
    <div className="hex scroll mono" ref={scrollRef}>
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((item) => {
          const start = item.index * PER_ROW;
          const slice = bytes.slice(start, start + PER_ROW);
          return (
            <div
              key={item.index}
              className="hex-row"
              style={{ transform: `translateY(${item.start}px)`, height: ROW_HEIGHT }}
            >
              <span className="hex-off" title={`File offset 0x${(fileOffset + start).toString(16).toUpperCase()}`}>
                {start.toString(16).toUpperCase().padStart(digits, "0")}
              </span>
              <span className="hex-bytes">
                {slice.map((b, i) => (
                  <span
                    key={i}
                    className={"hex-byte" + (inRange(start + i) ? " hl" : "") + (b === 0 ? " zero" : "")}
                    onClick={() => onPick(start + i)}
                  >
                    {b.toString(16).padStart(2, "0")}
                  </span>
                ))}
              </span>
              <span className="hex-ascii">
                {slice.map((b, i) => (
                  <span key={i} className={inRange(start + i) ? "hl" : undefined} onClick={() => onPick(start + i)}>
                    {printable(b)}
                  </span>
                ))}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
