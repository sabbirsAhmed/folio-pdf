import { useEffect, useState } from "react";
import {
  FolderOpen, Save, Printer, Undo2, Redo2, ChevronUp, ChevronDown, ZoomIn, ZoomOut, MousePointer2, Hand,
  Highlighter, StickyNote, PenLine, Type, LayoutGrid, FileText, PanelRight, Search, Share2,
} from "lucide-react";
import { useStore, type Tool } from "../store";
import { openFiles, saveDoc, printDoc, undo, redo, zoomBy, setZoom, goToPage } from "../actions";

const ZOOMS = [0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];

export function CommandBar() {
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null));
  const tool = useStore((s) => s.tool);
  const setTool = useStore((s) => s.setTool);
  const patchTab = useStore((s) => s.patchTab);
  const toggleTools = useStore((s) => s.toggleToolsPane);
  const toolsPane = useStore((s) => s.toolsPane);
  const setPanel = useStore((s) => s.setPanel);
  const openDialog = useStore((s) => s.openDialog);
  const [pageText, setPageText] = useState("1");
  useEffect(() => setPageText(String((t?.page ?? 0) + 1)), [t?.page]);
  if (!t) return null;
  const organize = t.mode === "organize";
  const tb = (id: Tool, icon: JSX.Element, label: string) => (
    <button className={`ibtn ${tool === id ? "on" : ""}`} title={label} aria-pressed={tool === id} onClick={() => setTool(tool === id ? "select" : id)} disabled={organize}>
      {icon}
    </button>
  );
  const zoomValue = t.fit ? t.fit : String(t.zoom);
  return (
    <div className="cmdbar">
      <button className="ibtn" title="Open (Ctrl+O)" onClick={openFiles}><FolderOpen size={17} /></button>
      <button className="ibtn" title="Save (Ctrl+S)" onClick={() => saveDoc()}><Save size={17} /></button>
      <button className="ibtn" title="Print (Ctrl+P)" onClick={printDoc}><Printer size={17} /></button>
      <button className="ibtn" title="Export" onClick={() => openDialog("export")}><Share2 size={17} /></button>
      <span className="sep" />
      <button className="ibtn" title="Undo (Ctrl+Z)" disabled={!t.info.can_undo} onClick={undo}><Undo2 size={17} /></button>
      <button className="ibtn" title="Redo (Ctrl+Y)" disabled={!t.info.can_redo} onClick={redo}><Redo2 size={17} /></button>
      <span className="sep" />
      {tb("select", <MousePointer2 size={17} />, "Select text and comments (V)")}
      {tb("hand", <Hand size={17} />, "Hand (H)")}
      {tb("highlight", <Highlighter size={17} />, "Highlight text (U)")}
      {tb("note", <StickyNote size={17} />, "Sticky note (N)")}
      {tb("ink", <PenLine size={17} />, "Draw (P)")}
      {tb("textbox", <Type size={17} />, "Text box (T)")}
      <span className="sep" />
      <div className="pagebox">
        <button className="ibtn" title="Previous page" disabled={organize || t.page === 0} onClick={() => goToPage(t.page - 1)}><ChevronUp size={17} /></button>
        <button className="ibtn" title="Next page" disabled={organize || t.page >= t.info.page_count - 1} onClick={() => goToPage(t.page + 1)}><ChevronDown size={17} /></button>
        <input
          type="text"
          aria-label="Page number"
          value={pageText}
          disabled={organize}
          onChange={(e) => setPageText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              const n = parseInt(pageText, 10);
              if (!isNaN(n)) goToPage(n - 1);
              (e.target as HTMLInputElement).blur();
            }
          }}
          onBlur={() => setPageText(String(t.page + 1))}
        />
        <span>of {t.info.page_count}</span>
      </div>
      <span className="sep" />
      <button className="ibtn" title="Zoom out (Ctrl+−)" disabled={organize} onClick={() => zoomBy(1 / 1.2)}><ZoomOut size={17} /></button>
      <span className="zoombox">
        <select
          className="sel"
          aria-label="Zoom"
          disabled={organize}
          value={ZOOMS.includes(t.zoom) || t.fit ? zoomValue : "custom"}
          onChange={(e) => {
            const v = e.target.value;
            if (v === "width" || v === "page") setZoom(v);
            else if (v !== "custom") setZoom(parseFloat(v));
          }}
        >
          <option value="width">Fit width</option>
          <option value="page">Fit page</option>
          {!t.fit && !ZOOMS.includes(t.zoom) && <option value="custom">{Math.round(t.zoom * 100)}%</option>}
          {ZOOMS.map((z) => <option key={z} value={String(z)}>{z * 100}%</option>)}
        </select>
      </span>
      <button className="ibtn" title="Zoom in (Ctrl+=)" disabled={organize} onClick={() => zoomBy(1.2)}><ZoomIn size={17} /></button>
      <span className="grow" />
      <button className={`ibtn ${!organize ? "on" : ""}`} title="Read and edit" onClick={() => patchTab(t.info.id, { mode: "view" })}><FileText size={17} /></button>
      <button className={`ibtn ${organize ? "on" : ""}`} title="Organize pages" onClick={() => patchTab(t.info.id, { mode: "organize" })}><LayoutGrid size={17} /></button>
      <span className="sep" />
      <button className="ibtn" title="Find (Ctrl+F)" disabled={organize} onClick={() => { setPanel("search"); setTimeout(() => document.getElementById("search-input")?.focus(), 30); }}><Search size={17} /></button>
      <button className={`ibtn ${toolsPane ? "on" : ""}`} title="All tools" onClick={toggleTools}><PanelRight size={17} /></button>
    </div>
  );
}
