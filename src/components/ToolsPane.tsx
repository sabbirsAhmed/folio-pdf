import {
  Highlighter, PenLine, StickyNote, Stamp, Signature, Type, FileEdit, ImagePlus, LayoutGrid, Combine, FormInput,
  EraserIcon, Lock, Minimize2, FileOutput, ScanText, Droplets, Hash, Info, Printer, Crop, Layers, ShieldCheck, FileStack,
} from "lucide-react";
import { useStore, type Tool } from "../store";
import { printDoc } from "../actions";

type Item = { label: string; icon: JSX.Element; tool?: Tool; dialog?: string; run?: () => void; danger?: boolean };

export function ToolsPane() {
  const tool = useStore((s) => s.tool);
  const setTool = useStore((s) => s.setTool);
  const openDialog = useStore((s) => s.openDialog);
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null));
  const patchTab = useStore((s) => s.patchTab);
  if (!t) return null;
  const toView = () => t.mode !== "view" && patchTab(t.info.id, { mode: "view" });

  const groups: [string, Item[]][] = [
    ["Comment", [
      { label: "Highlight & mark up", icon: <Highlighter size={17} />, tool: "highlight" },
      { label: "Sticky note", icon: <StickyNote size={17} />, tool: "note" },
      { label: "Text box", icon: <Type size={17} />, tool: "textbox" },
      { label: "Draw & shapes", icon: <PenLine size={17} />, tool: "ink" },
      { label: "Stamp", icon: <Stamp size={17} />, tool: "stamp" },
    ]],
    ["Fill & sign", [
      { label: "Sign", icon: <Signature size={17} />, tool: "sign" },
      { label: "Fill in text", icon: <Type size={17} />, tool: "addtext" },
    ]],
    ["Edit", [
      { label: "Edit text & images", icon: <FileEdit size={17} />, tool: "edit" },
      { label: "Add image", icon: <ImagePlus size={17} />, tool: "addimage" },
      { label: "Watermark", icon: <Droplets size={17} />, dialog: "watermark" },
      { label: "Header, footer & page numbers", icon: <Hash size={17} />, dialog: "header-footer" },
      { label: "Crop pages", icon: <Crop size={17} />, tool: "crop" },
    ]],
    ["Pages", [
      { label: "Organize pages", icon: <LayoutGrid size={17} />, run: () => patchTab(t.info.id, { mode: "organize" }) },
      { label: "Combine files", icon: <Combine size={17} />, dialog: "combine" },
      { label: "Split document", icon: <FileStack size={17} />, dialog: "split" },
    ]],
    ["Forms", [
      { label: "Prepare form", icon: <FormInput size={17} />, tool: "field" },
      { label: "Form data & flatten", icon: <Layers size={17} />, dialog: "form-tools" },
    ]],
    ["Protect", [
      { label: "Redact", icon: <EraserIcon size={17} />, tool: "redact", danger: true },
      { label: "Password protect", icon: <Lock size={17} />, dialog: "protect" },
      { label: "Remove hidden information", icon: <ShieldCheck size={17} />, dialog: "sanitize" },
    ]],
    ["Convert", [
      { label: "Export to Word, Excel, images", icon: <FileOutput size={17} />, dialog: "export" },
      { label: "Recognise text (OCR)", icon: <ScanText size={17} />, dialog: "ocr" },
      { label: "Reduce file size", icon: <Minimize2 size={17} />, dialog: "compress" },
    ]],
    ["Document", [
      { label: "Properties", icon: <Info size={17} />, dialog: "properties" },
      { label: "Print", icon: <Printer size={17} />, run: printDoc },
    ]],
  ];

  return (
    <aside className="tools" aria-label="All tools">
      {groups.map(([title, items]) => (
        <section key={title}>
          <h3>{title}</h3>
          {items.map((it) => (
            <button
              key={it.label}
              className={`tool-row ${it.tool && tool === it.tool && t.mode === "view" ? "on" : ""} ${it.danger ? "danger" : ""}`}
              onClick={() => {
                if (it.tool) { toView(); setTool(it.tool); }
                else if (it.dialog) openDialog(it.dialog);
                else it.run?.();
              }}
            >
              {it.icon}
              <span>{it.label}</span>
            </button>
          ))}
        </section>
      ))}
    </aside>
  );
}
