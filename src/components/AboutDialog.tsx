import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { X } from "lucide-react";
import logo from "../assets/logo.png";

/** About JD IDE: who made it, what it is built on, and that it is free. */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => { getVersion().then(setVersion).catch(() => setVersion(null)); }, []);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); onClose(); } };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal about-dialog" role="dialog" aria-label="About JD IDE" onMouseDown={(event) => event.stopPropagation()}>
      <button className="icon-btn about-close" onClick={onClose} aria-label="Close"><X size={16} /></button>
      <div className="about-head">
        <img src={logo} alt="" draggable={false} />
        <div>
          <h2>JD IDE</h2>
          <div className="muted small">{version ? `Version ${version}` : "Jade Dynasty data editor"}</div>
        </div>
      </div>
      <p>An editor for Jade Dynasty (Zhu Xian) game data: elements.data, tasks, shops, spawns, map areas, AI, server scripts and more, for both the game client and the server.</p>
      <p>Made by <b>mrosenov</b>, with extensive help from <b>Claude</b> (Anthropic) and <b>Codex</b> (OpenAI).</p>
      <p>Built with the help of the available source code of the game client, the server and the official tools, studied so that every file is read and written exactly as the game itself does.</p>
      <div className="about-free">This product is <b>FREE</b>.</div>
      <p className="muted small">Jade Dynasty (Zhu Xian) belongs to its publishers; JD IDE is an independent tool and is not affiliated with them. It uses open-source software including Tauri, React, luaparse and fengari.</p>
      <footer><span className="spacer" /><button className="btn primary" onClick={onClose}>Close</button></footer>
    </div>
  </div>;
}
