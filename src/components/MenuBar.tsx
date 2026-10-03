import { useEffect, useRef, useState } from "react";
import { Check, ChevronRight, type LucideIcon } from "lucide-react";

export interface MenuItem {
  label: string;
  icon?: LucideIcon;
  /** Shown on the right, e.g. "Ctrl+O" (the shortcut itself is handled elsewhere). */
  shortcut?: string;
  onSelect?: () => void;
  disabled?: boolean;
  /** Shows a tick (the panel is open). */
  checked?: boolean;
  /** Why the item is disabled, or what it does. */
  title?: string;
  /** Small counts after the label, e.g. errors and warnings. */
  badges?: { text: string; tone: "error" | "warning" }[];
  submenu?: MenuItem[];
}

export type MenuEntry = MenuItem | "separator";

export interface Menu {
  label: string;
  /** Alt+key opens the menu. */
  accessKey: string;
  items: MenuEntry[];
}

/** A desktop-style menu bar: click a menu, hover to switch, Esc or a click outside closes. */
export function MenuBar({ menus }: { menus: Menu[] }) {
  const [open, setOpen] = useState<number | null>(null);
  const [sub, setSub] = useState<number | null>(null);
  const bar = useRef<HTMLDivElement>(null);

  const close = () => {
    setOpen(null);
    setSub(null);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey && !e.ctrlKey && !e.shiftKey) {
        const i = menus.findIndex((m) => m.accessKey.toLowerCase() === e.key.toLowerCase());
        if (i >= 0) {
          e.preventDefault();
          setOpen((o) => (o === i ? null : i));
          setSub(null);
          return;
        }
      }
      if (open !== null && e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        close();
      }
    };
    const onDown = (e: MouseEvent) => open !== null && !bar.current?.contains(e.target as Node) && close();
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", onDown);
    };
  }, [menus, open]);

  const select = (item: MenuItem) => {
    if (item.disabled || item.submenu) return;
    close();
    item.onSelect?.();
  };

  const row = (item: MenuItem, i: number, nested: boolean) => {
    const Icon = item.icon;
    return (
      <div key={item.label} className="menu-item-wrap" onMouseEnter={() => !nested && setSub(item.submenu ? i : null)}>
        <button
          className={"menu-item" + (item.disabled ? " disabled" : "")}
          role="menuitem"
          aria-disabled={item.disabled}
          aria-haspopup={item.submenu ? "menu" : undefined}
          onClick={() => (item.submenu ? setSub(i) : select(item))}
          title={item.title}
        >
          <span className="menu-icon">{item.checked ? <Check size={14} /> : Icon ? <Icon size={14} /> : null}</span>
          <span className="menu-label">
            <span className="truncate">{item.label}</span>
            {item.badges?.map((b) => (
              <span key={b.tone} className={`menu-badge ${b.tone}`}>
                {b.text}
              </span>
            ))}
          </span>
          {item.shortcut && <span className="menu-shortcut">{item.shortcut}</span>}
          {item.submenu && <ChevronRight size={13} className="menu-more" />}
        </button>
        {item.submenu && sub === i && !nested && (
          <div className="menu-pop submenu" role="menu">
            {item.submenu.map((s, j) => row(s, j, true))}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="menubar" ref={bar} role="menubar">
      {menus.map((m, i) => (
        <div key={m.label} className="menu-root">
          <button
            className={"menu-title" + (open === i ? " open" : "")}
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={open === i}
            onClick={() => {
              setOpen(open === i ? null : i);
              setSub(null);
            }}
            onMouseEnter={() => open !== null && open !== i && (setOpen(i), setSub(null))}
            title={`${m.label} (Alt+${m.accessKey.toUpperCase()})`}
          >
            {m.label}
          </button>
          {open === i && (
            <div className="menu-pop" role="menu">
              {m.items.map((item, j) => (item === "separator" ? <div key={`sep${j}`} className="menu-sep" role="separator" /> : row(item, j, false)))}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
