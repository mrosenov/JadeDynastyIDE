import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import type { Tab } from "../tabs";

export interface TabLabel {
  title: string;
  subtitle: string;
  /** Item icon URL, if any. */
  icon?: string;
  /** The record has unsaved edits. */
  changed?: boolean;
}

interface Props {
  tabs: Tab[];
  active: number | null;
  label: (tab: Tab) => TabLabel;
  onActivate: (id: number) => void;
  onPin: (id: number) => void;
  onClose: (id: number) => void;
}

export function TabBar({ tabs, active, label, onActivate, onPin, onClose }: Props) {
  const strip = useRef<HTMLDivElement>(null);

  // Keep the active tab in view.
  useEffect(() => {
    strip.current?.querySelector(".tab.active")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [active, tabs.length]);

  if (!tabs.length) return null;

  return (
    <div
      className="tabbar"
      ref={strip}
      role="tablist"
      onWheel={(e) => {
        // Scroll the strip sideways with the mouse wheel.
        if (strip.current && e.deltaY) strip.current.scrollLeft += e.deltaY;
      }}
    >
      {tabs.map((tab, i) => {
        const { title, subtitle, icon, changed } = label(tab);
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={tab.id === active}
            className={"tab" + (tab.id === active ? " active" : "") + (tab.pinned ? "" : " preview")}
            onClick={() => onActivate(tab.id)}
            onDoubleClick={() => onPin(tab.id)}
            onAuxClick={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                onClose(tab.id);
              }
            }}
            onMouseDown={(e) => e.button === 1 && e.preventDefault()}
            title={
              `${title}\n${subtitle}` +
              (tab.pinned ? "" : "\nPreview tab: double-click to keep it open") +
              (i < 9 ? `\nCtrl+${i + 1}` : "")
            }
          >
            {icon && <img className="tab-icon" src={icon} alt="" draggable={false} />}
            <span className="tab-text">
              <span className="tab-title">
                {changed && <span className="changed-dot" title="Changed (not saved yet)" />}
                {title}
              </span>
              <span className="tab-subtitle">{subtitle}</span>
            </span>
            <button
              className="tab-close"
              onClick={(e) => {
                e.stopPropagation();
                onClose(tab.id);
              }}
              aria-label="Close tab"
              title="Close (Ctrl+W)"
            >
              <X size={13} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
