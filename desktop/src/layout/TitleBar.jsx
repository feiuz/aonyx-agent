import { Minus, Square, X } from "lucide-react";
import logo from "../assets/logo.png";

// Frameless window: custom controls driven by the Tauri window API (exposed via
// withGlobalTauri). No-ops gracefully outside Tauri (browser preview).
function getWin() {
  try {
    const w = window.__TAURI__?.window;
    return w?.getCurrentWindow ? w.getCurrentWindow() : null;
  } catch {
    return null;
  }
}

export default function TitleBar() {
  const min = () => getWin()?.minimize?.();
  const max = () => getWin()?.toggleMaximize?.();
  const close = () => getWin()?.close?.();

  return (
    <div
      data-tauri-drag-region
      className="flex items-center justify-between h-9 pl-3 flex-shrink-0 bg-aonyx-100 dark:bg-aonyx-950 border-b border-aonyx-200 dark:border-aonyx-800 select-none"
    >
      <div data-tauri-drag-region className="flex items-center gap-2 pointer-events-none">
        <img src={logo} className="w-4 h-4 rounded-full" alt="" />
        <span className="text-[11px] font-cond uppercase tracking-[0.18em] text-aonyx-600 dark:text-aonyx-300">
          Aonyx&nbsp;Agent
        </span>
      </div>
      <div className="flex items-center">
        <button onClick={min} className="titlebar-btn" aria-label="Minimize">
          <Minus className="w-4 h-4" strokeWidth={1.75} />
        </button>
        <button onClick={max} className="titlebar-btn" aria-label="Maximize">
          <Square className="w-3 h-3" strokeWidth={1.75} />
        </button>
        <button onClick={close} className="titlebar-btn titlebar-btn-close" aria-label="Close">
          <X className="w-4 h-4" strokeWidth={1.75} />
        </button>
      </div>
    </div>
  );
}
