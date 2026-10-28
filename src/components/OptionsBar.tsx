import { useStore, type Tool } from "../store";
import { api } from "../api";
import { mutate } from "../actions";

const MARKUP = ["#ffd43b", "#8ce99a", "#74c0fc", "#faa2c1", "#ff8787"];
const PEN = ["#d6336c", "#1c7ed6", "#212529", "#2f9e44", "#7048e8", "#f08c00"];
export const STAMPS: { label: string; color: string }[] = [
  { label: "APPROVED", color: "#2b8a3e" },
  { label: "REVIEWED", color: "#1864ab" },
  { label: "RECEIVED", color: "#1864ab" },
  { label: "DRAFT", color: "#868e96" },
  { label: "FINAL", color: "#2b8a3e" },
  { label: "CONFIDENTIAL", color: "#c92a2a" },
  { label: "NOT APPROVED", color: "#c92a2a" },
  { label: "VOID", color: "#c92a2a" },
  { label: "FOR COMMENT", color: "#e67700" },
];
const FIELD_KINDS: [string, string][] = [
  ["text", "Text"], ["multiline", "Paragraph"], ["checkbox", "Checkbox"], ["radio", "Radio"],
  ["combo", "Dropdown"], ["list", "List"], ["signature", "Signature"],
];

const TOOL_NAMES: Partial<Record<Tool, string>> = {
  highlight: "Highlight", underline: "Underline", strikeout: "Strikethrough", squiggly: "Squiggly",
  note: "Sticky note", textbox: "Text box", ink: "Draw", rect: "Rectangle", ellipse: "Ellipse", line: "Line", arrow: "Arrow",
  stamp: "Stamp", sign: "Sign", addtext: "Add text", addimage: "Add image", edit: "Edit text & images",
  field: "Add form field", redact: "Redact", crop: "Crop pages", hand: "Hand", select: "Select",
};

function Swatches({ colors, value, onPick }: { colors: string[]; value: string; onPick: (c: string) => void }) {
  return (
    <div className="swatches" role="radiogroup" aria-label="Colour">
      {colors.map((c) => (
        <button key={c} role="radio" aria-checked={value === c} aria-label={c} className={`swatch ${value === c ? "on" : ""}`} style={{ background: c }} onClick={() => onPick(c)} />
      ))}
      <input type="color" aria-label="Custom colour" value={value} onChange={(e) => onPick(e.target.value)} style={{ width: 24, height: 22, border: "none", background: "none", padding: 0 }} />
    </div>
  );
}

export function OptionsBar() {
  const tool = useStore((s) => s.tool);
  const opts = useStore((s) => s.opts);
  const setOpts = useStore((s) => s.setOpts);
  const setTool = useStore((s) => s.setTool);
  const openDialog = useStore((s) => s.openDialog);
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null));
  const patchTab = useStore((s) => s.patchTab);
  if (!t) return <div />;
  const name = TOOL_NAMES[tool];
  const markup = ["highlight", "underline", "strikeout", "squiggly"].includes(tool);
  const pen = ["ink", "rect", "ellipse", "line", "arrow"].includes(tool);
  const text = ["textbox", "addtext", "note"].includes(tool);

  let body: JSX.Element | null = null;
  if (markup) {
    body = (
      <>
        <div className="seg">
          {(["highlight", "underline", "strikeout", "squiggly"] as Tool[]).map((m) => (
            <button key={m} className={tool === m ? "on" : ""} onClick={() => setTool(m)}>{TOOL_NAMES[m]}</button>
          ))}
        </div>
        <Swatches colors={MARKUP} value={opts.markupColor} onPick={(c) => setOpts({ markupColor: c })} />
        <span>Drag across text to mark it.</span>
      </>
    );
  } else if (pen) {
    body = (
      <>
        <div className="seg">
          {(["ink", "line", "arrow", "rect", "ellipse"] as Tool[]).map((m) => (
            <button key={m} className={tool === m ? "on" : ""} onClick={() => setTool(m)}>{TOOL_NAMES[m]}</button>
          ))}
        </div>
        <Swatches colors={PEN} value={opts.color} onPick={(c) => setOpts({ color: c })} />
        <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
          Thickness
          <input type="range" min={0.5} max={12} step={0.5} value={opts.width} onChange={(e) => setOpts({ width: parseFloat(e.target.value) })} />
          <span style={{ width: 28 }}>{opts.width}pt</span>
        </label>
      </>
    );
  } else if (text) {
    body = (
      <>
        <Swatches colors={tool === "note" ? MARKUP : ["#212529", ...PEN.slice(0, 5)]} value={tool === "note" ? opts.markupColor : opts.color} onPick={(c) => setOpts(tool === "note" ? { markupColor: c } : { color: c })} />
        {tool !== "note" && (
          <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
            Size
            <input type="number" className="field-input" style={{ width: 60 }} min={4} max={144} value={opts.fontSize} onChange={(e) => setOpts({ fontSize: Math.max(4, parseFloat(e.target.value) || 12) })} />
          </label>
        )}
        <span>{tool === "note" ? "Click where the note belongs." : tool === "textbox" ? "Drag a box, then type." : "Click where the text starts, then type."}</span>
      </>
    );
  } else if (tool === "stamp") {
    body = (
      <>
        {STAMPS.map((s) => (
          <button key={s.label} className={`chip ${opts.stampLabel === s.label ? "on" : ""}`} onClick={() => setOpts({ stampLabel: s.label })}>{s.label}</button>
        ))}
        <span>Click to place, or drag to size.</span>
      </>
    );
  } else if (tool === "sign") {
    body = (
      <>
        {opts.signature ? <img src={opts.signature} alt="Your signature" style={{ height: 26, background: "#fff", borderRadius: 3, padding: "0 4px" }} /> : <span>No signature yet.</span>}
        <button className="btn" onClick={() => openDialog("signature", { target: "signature" })}>{opts.signature ? "Change signature" : "Create signature"}</button>
        <button className="btn" onClick={() => openDialog("signature", { target: "initials" })}>{opts.initials ? "Change initials" : "Create initials"}</button>
        {opts.signature && <span>Click or drag on the page to place it.</span>}
      </>
    );
  } else if (tool === "field") {
    body = (
      <>
        {FIELD_KINDS.map(([k, l]) => (
          <button key={k} className={`chip ${opts.fieldKind === k ? "on" : ""}`} onClick={() => setOpts({ fieldKind: k })}>{l}</button>
        ))}
        <span>Drag to draw the field.</span>
      </>
    );
  } else if (tool === "redact") {
    const n = t.redactions.length;
    body = (
      <>
        <span>Drag over content to mark it. {n ? `${n} area${n > 1 ? "s" : ""} marked.` : ""}</span>
        <button className="btn" onClick={() => openDialog("redact-search")}>Find text to redact</button>
        <button className="btn" disabled={!n} onClick={() => patchTab(t.info.id, { redactions: [] })}>Clear marks</button>
        <button
          className="btn danger"
          disabled={!n}
          onClick={async () => {
            const areas = t.redactions;
            const ok = await mutate("Applying redactions", (id) => api.applyRedactions(id, areas, "#000000"));
            if (ok) {
              patchTab(t.info.id, { redactions: [] });
              useStore.getState().toast("Redactions applied. The marked content was removed from the file.");
            }
          }}
        >
          Apply redactions
        </button>
      </>
    );
  } else if (tool === "edit" || tool === "addimage") {
    body = (
      <>
        <div className="seg">
          <button className={tool === "edit" ? "on" : ""} onClick={() => setTool("edit")}>Edit</button>
          <button onClick={() => setTool("addtext")}>Add text</button>
          <button className={tool === "addimage" ? "on" : ""} onClick={() => setTool("addimage")}>Add image</button>
        </div>
        <span>{tool === "edit" ? "Double-click text to change it. Drag to move. Delete removes the selection." : "Drag a box where the image goes."}</span>
      </>
    );
  } else if (tool === "crop") {
    body = (
      <>
        <span>Drag the area to keep.</span>
        <label className="check"><input type="checkbox" checked={opts.cropAll} onChange={(e) => setOpts({ cropAll: e.target.checked })} /> Apply to all pages</label>
      </>
    );
  }

  if (!body) return <div />;
  return (
    <div className="optbar">
      <span className="label">{name}</span>
      {body}
      <span style={{ flex: 1 }} />
      <button className="ibtn small" title="Done (Esc)" onClick={() => setTool("select")}>Done</button>
    </div>
  );
}
