import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useStore } from "./store";
import { api } from "./api";
import { openAny, openFiles, saveDoc, undo, redo, printDoc, zoomBy, setZoom, closeDoc, createBlank, goToPage } from "./actions";
import { TabStrip } from "./components/TabStrip";
import { CommandBar } from "./components/CommandBar";
import { OptionsBar } from "./components/OptionsBar";
import { SideRail, SidePanel } from "./components/Sidebar";
import { Viewer } from "./components/Viewer";
import { ToolsPane } from "./components/ToolsPane";
import { Organize } from "./components/Organize";
import { Home } from "./components/Home";
import { Dialogs } from "./components/Dialogs";
import { StatusBar, Toasts } from "./components/StatusBar";

export default function App() {
  const active = useStore((s) => s.active);
  const mode = useStore((s) => (s.active != null ? s.tabs[s.active]?.mode : undefined));
  const panel = useStore((s) => s.panel);
  const toolsPane = useStore((s) => s.toolsPane);
  const hasTabs = useStore((s) => s.order.length > 0);

  // Startup: author name, files passed on the command line / via "Open with".
  useEffect(() => {
    api.userName().then((n) => useStore.getState().setAuthor(localStorage.getItem("folio.author") || n)).catch(() => undefined);
    api.takePendingFiles().then((f) => { if (f.length) openAny(f); }).catch(() => undefined);
    const un1 = listen<string[]>("open-files", (e) => openAny(e.payload));
    const un2 = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop") openAny(e.payload.paths);
    });
    const un3 = getCurrentWindow().onCloseRequested(async (e) => {
      const dirty = Object.values(useStore.getState().tabs).filter((t) => t.info.dirty);
      if (dirty.length) {
        e.preventDefault();
        useStore.getState().openDialog("quit", { count: dirty.length });
      }
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
      un3.then((f) => f());
    };
  }, []);

  // Window title follows the active document.
  useEffect(() => {
    const t = active != null ? useStore.getState().tabs[active] : null;
    getCurrentWindow().setTitle(t ? `${t.info.name} — Folio PDF` : "Folio PDF").catch(() => undefined);
  });

  // Global shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      const target = e.target as HTMLElement;
      const typing = target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable);
      const s = useStore.getState();
      if (mod && e.key.toLowerCase() === "o") { e.preventDefault(); openFiles(); return; }
      if (mod && e.key.toLowerCase() === "n") { e.preventDefault(); createBlank(); return; }
      if (s.dialog) return;
      if (mod && e.key.toLowerCase() === "s") { e.preventDefault(); saveDoc(undefined, e.shiftKey); return; }
      if (mod && e.key.toLowerCase() === "p") { e.preventDefault(); printDoc(); return; }
      if (mod && e.key.toLowerCase() === "w") { e.preventDefault(); if (s.active != null) closeDoc(s.active); return; }
      if (mod && e.key.toLowerCase() === "f") { e.preventDefault(); if (s.panel !== "search") s.setPanel("search"); setTimeout(() => document.getElementById("search-input")?.focus(), 30); return; }
      if (typing) return;
      if (mod && e.key.toLowerCase() === "z" && !e.shiftKey) { e.preventDefault(); undo(); return; }
      if (mod && (e.key.toLowerCase() === "y" || (e.key.toLowerCase() === "z" && e.shiftKey))) { e.preventDefault(); redo(); return; }
      if (mod && (e.key === "=" || e.key === "+")) { e.preventDefault(); zoomBy(1.2); return; }
      if (mod && e.key === "-") { e.preventDefault(); zoomBy(1 / 1.2); return; }
      if (mod && e.key === "0") { e.preventDefault(); setZoom("page"); return; }
      if (mod && e.key === "1") { e.preventDefault(); setZoom(1); return; }
      if (mod && e.key === "2") { e.preventDefault(); setZoom("width"); return; }
      if (e.key === "Escape") { s.setTool("select"); return; }
      const t = s.active != null ? s.tabs[s.active] : null;
      if (t && t.mode === "view" && !mod) {
        if (e.key === "PageDown" || (e.key === "ArrowRight" && e.altKey)) { e.preventDefault(); goToPage(t.page + 1); }
        if (e.key === "PageUp" || (e.key === "ArrowLeft" && e.altKey)) { e.preventDefault(); goToPage(t.page - 1); }
        if (e.key === "Home" && e.ctrlKey) goToPage(0);
        if (e.key === "End" && e.ctrlKey) goToPage(t.info.page_count - 1);
        const keymap: Record<string, Parameters<typeof s.setTool>[0]> = { v: "select", h: "hand", u: "highlight", n: "note", p: "ink", t: "textbox", r: "rect" };
        if (keymap[e.key]) s.setTool(keymap[e.key]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="app">
      <TabStrip />
      {hasTabs && active != null ? <CommandBar /> : <div />}
      {hasTabs && active != null ? (
        <div className="workspace">
          <SideRail />
          {panel && mode === "view" ? <SidePanel /> : <div />}
          <div style={{ display: "grid", gridTemplateRows: "auto 1fr", minHeight: 0, minWidth: 0 }}>
            {mode === "view" ? <OptionsBar /> : <div />}
            {mode === "organize" ? <Organize /> : <Viewer key={active} />}
          </div>
          {toolsPane ? <ToolsPane /> : <div />}
        </div>
      ) : (
        <Home />
      )}
      <StatusBar />
      <Dialogs />
      <Toasts />
    </div>
  );
}
