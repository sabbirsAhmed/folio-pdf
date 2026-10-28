import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { ArrowDown, ArrowUp, X } from "lucide-react";
import { useStore, activeTab } from "../store";
import { api, errMsg, type Rect, type Security } from "../api";
import {
  openPath, saveDoc, closeDoc, mutate, task, pickSavePath, pickFolder, stem, baseName, PDF_FILTER, IMAGE_FILTER, dirName,
} from "../actions";
import { ocrPages } from "../ocr";
import { lineRects } from "./PageView";

function Modal({ title, children, footer, wide, onClose }: { title: string; children: React.ReactNode; footer: React.ReactNode; wide?: boolean; onClose: () => void }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`dialog ${wide ? "wide" : ""}`} role="dialog" aria-modal aria-label={title}>
        <header>{title}</header>
        <div className="body">{children}</div>
        <footer>{footer}</footer>
      </div>
    </div>
  );
}

const Row = ({ label, children }: { label: string; children: React.ReactNode }) => (
  <label className="form-row"><span>{label}</span>{children}</label>
);

export function Dialogs() {
  const d = useStore((s) => s.dialog);
  const close = useStore((s) => s.closeDialog);
  if (!d) return null;
  const p = (d.props ?? {}) as Record<string, any>;
  switch (d.kind) {
    case "password": return <PasswordDialog path={p.path} retry={p.retry} close={close} />;
    case "unsaved": return <UnsavedDialog id={p.id} close={close} />;
    case "quit": return <QuitDialog count={p.count} close={close} />;
    case "confirm": return <ConfirmDialog {...(p as any)} close={close} />;
    case "prompt": return <PromptDialog {...(p as any)} close={close} />;
    case "signature": return <SignatureDialog target={p.target} close={close} />;
    case "new-field": return <NewFieldDialog page={p.page} kind={p.kind} rect={p.rect} close={close} />;
    case "watermark": return <WatermarkDialog close={close} />;
    case "header-footer": return <HeaderFooterDialog close={close} />;
    case "combine": return <CombineDialog close={close} />;
    case "split": return <SplitDialog close={close} />;
    case "form-tools": return <FormToolsDialog close={close} />;
    case "protect": return <ProtectDialog close={close} />;
    case "sanitize": return <SanitizeDialog close={close} />;
    case "export": return <ExportDialog close={close} />;
    case "ocr": return <OcrDialog close={close} />;
    case "compress": return <CompressDialog close={close} />;
    case "properties": return <PropertiesDialog close={close} />;
    case "redact-search": return <RedactSearchDialog close={close} />;
    default: return null;
  }
}

// ----------------------------------------------------------------- basics

function PasswordDialog({ path, retry, close }: { path: string; retry: boolean; close: () => void }) {
  const [pw, setPw] = useState("");
  const ok = () => { close(); openPath(path, pw); };
  return (
    <Modal title="Password required" onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" onClick={ok} disabled={!pw}>Open</button></>}>
      <p style={{ margin: 0 }}>{baseName(path)} is protected. Enter its password to open it.</p>
      {retry && <p style={{ margin: 0, color: "var(--danger)" }}>That password did not work. Try again.</p>}
      <Row label="Password"><input type="password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} onKeyDown={(e) => e.key === "Enter" && pw && ok()} /></Row>
    </Modal>
  );
}

function UnsavedDialog({ id, close }: { id: number; close: () => void }) {
  const t = useStore((s) => s.tabs[id]);
  if (!t) return null;
  return (
    <Modal title={`Save changes to ${t.info.name}?`} onClose={close} footer={<>
      <button className="btn" onClick={() => { close(); closeDoc(id, true); }}>Don't save</button>
      <span className="grow" />
      <button className="btn" onClick={close}>Cancel</button>
      <button className="btn primary" onClick={async () => { close(); if (await saveDoc(id)) closeDoc(id, true); }}>Save</button>
    </>}>
      <p style={{ margin: 0 }}>Your changes will be lost if you close without saving.</p>
    </Modal>
  );
}

function QuitDialog({ count, close }: { count: number; close: () => void }) {
  const quit = () => getCurrentWindow().destroy();
  return (
    <Modal title="Quit with unsaved changes?" onClose={close} footer={<>
      <button className="btn" onClick={quit}>Quit without saving</button>
      <span className="grow" />
      <button className="btn" onClick={close}>Cancel</button>
      <button className="btn primary" onClick={async () => {
        close();
        for (const t of Object.values(useStore.getState().tabs)) if (t.info.dirty && !(await saveDoc(t.info.id))) return;
        quit();
      }}>Save all and quit</button>
    </>}>
      <p style={{ margin: 0 }}>{count} document{count > 1 ? "s have" : " has"} unsaved changes.</p>
    </Modal>
  );
}

function ConfirmDialog({ title, body, ok, danger, onOk, close }: { title: string; body: string; ok: string; danger?: boolean; onOk: () => void; close: () => void }) {
  return (
    <Modal title={title} onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className={`btn ${danger ? "danger" : "primary"}`} autoFocus onClick={() => { close(); onOk(); }}>{ok}</button></>}>
      <p style={{ margin: 0 }}>{body}</p>
    </Modal>
  );
}

function PromptDialog({ title, label, value, onOk, close }: { title: string; label: string; value: string; onOk: (v: string) => void; close: () => void }) {
  const [v, setV] = useState(value ?? "");
  const ok = () => { if (!v.trim()) return; close(); onOk(v.trim()); };
  return (
    <Modal title={title} onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" onClick={ok} disabled={!v.trim()}>OK</button></>}>
      <Row label={label}><input type="text" autoFocus value={v} onChange={(e) => setV(e.target.value)} onFocus={(e) => e.target.select()} onKeyDown={(e) => e.key === "Enter" && ok()} /></Row>
    </Modal>
  );
}

// ----------------------------------------------------------------- signature

function SignatureDialog({ target, close }: { target: "signature" | "initials"; close: () => void }) {
  const setOpts = useStore((s) => s.setOpts);
  const author = useStore((s) => s.author);
  const [mode, setMode] = useState<"draw" | "type" | "image">("draw");
  const [typed, setTyped] = useState(target === "initials" ? author.split(/\s+/).map((w) => w[0]).join("").toUpperCase() : author);
  const [img, setImg] = useState<string | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const drawing = useRef(false);
  const [hasInk, setHasInk] = useState(false);

  useEffect(() => {
    const c = canvas.current;
    if (!c || mode !== "draw") return;
    const r = c.getBoundingClientRect();
    c.width = r.width * 2;
    c.height = r.height * 2;
    const ctx = c.getContext("2d")!;
    ctx.scale(2, 2);
    ctx.lineWidth = 2.4;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = "#10255e";
    setHasInk(false);
  }, [mode]);

  const pos = (e: React.PointerEvent) => {
    const r = canvas.current!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top] as const;
  };

  /** Trim transparent margins and export a PNG data URL. */
  const trimmed = (c: HTMLCanvasElement) => {
    const ctx = c.getContext("2d")!;
    const { width, height } = c;
    const data = ctx.getImageData(0, 0, width, height).data;
    let x0 = width, y0 = height, x1 = 0, y1 = 0;
    for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
      if (data[(y * width + x) * 4 + 3] > 10) { if (x < x0) x0 = x; if (y < y0) y0 = y; if (x > x1) x1 = x; if (y > y1) y1 = y; }
    }
    if (x1 <= x0 || y1 <= y0) return null;
    const pad = 8;
    const out = document.createElement("canvas");
    out.width = x1 - x0 + pad * 2;
    out.height = y1 - y0 + pad * 2;
    out.getContext("2d")!.drawImage(c, x0 - pad, y0 - pad, out.width, out.height, 0, 0, out.width, out.height);
    return out.toDataURL("image/png");
  };

  const fromTyped = () => {
    const c = document.createElement("canvas");
    c.width = 1200; c.height = 300;
    const ctx = c.getContext("2d")!;
    ctx.fillStyle = "#10255e";
    ctx.font = `120px "Segoe Script", "Brush Script MT", "Snell Roundhand", cursive`;
    ctx.textBaseline = "middle";
    ctx.fillText(typed, 20, 150);
    return trimmed(c);
  };

  const save = () => {
    const url = mode === "draw" ? trimmed(canvas.current!) : mode === "type" ? fromTyped() : img;
    if (!url) return;
    setOpts(target === "signature" ? { signature: url } : { initials: url });
    useStore.getState().setTool("sign");
    close();
  };

  return (
    <Modal wide title={target === "signature" ? "Create signature" : "Create initials"} onClose={close} footer={<>
      <span className="grow muted">Stored on this computer only.</span>
      <button className="btn" onClick={close}>Cancel</button>
      <button className="btn primary" onClick={save} disabled={mode === "draw" ? !hasInk : mode === "type" ? !typed.trim() : !img}>Use this {target}</button>
    </>}>
      <div className="seg">
        {(["draw", "type", "image"] as const).map((m) => <button key={m} className={mode === m ? "on" : ""} onClick={() => setMode(m)}>{m === "draw" ? "Draw" : m === "type" ? "Type" : "Image"}</button>)}
      </div>
      {mode === "draw" && (
        <>
          <canvas
            ref={canvas}
            className="sigpad"
            onPointerDown={(e) => { drawing.current = true; const [x, y] = pos(e); const ctx = canvas.current!.getContext("2d")!; ctx.beginPath(); ctx.moveTo(x, y); ctx.lineTo(x + 0.1, y); ctx.stroke(); canvas.current!.setPointerCapture(e.pointerId); setHasInk(true); }}
            onPointerMove={(e) => { if (!drawing.current) return; const [x, y] = pos(e); const ctx = canvas.current!.getContext("2d")!; ctx.lineTo(x, y); ctx.stroke(); }}
            onPointerUp={() => { drawing.current = false; }}
          />
          <div><button className="btn" onClick={() => { const c = canvas.current!; c.getContext("2d")!.clearRect(0, 0, c.width, c.height); setHasInk(false); }}>Clear</button></div>
        </>
      )}
      {mode === "type" && (
        <>
          <Row label="Name"><input type="text" value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus /></Row>
          <div className="sig-typed" style={{ background: "#fff", borderRadius: 4, padding: "10px 16px", minHeight: 80 }}>{typed}</div>
        </>
      )}
      {mode === "image" && (
        <>
          <button className="btn" onClick={async () => {
            const p = await openFileDialog({ filters: IMAGE_FILTER });
            if (typeof p !== "string") return;
            try {
              setImg(await api.readImageDataUrl(p));
            } catch (e) {
              useStore.getState().toast(errMsg(e), "error");
            }
          }}>Choose image…</button>
          {img && <img src={img} alt="Signature" style={{ maxHeight: 120, maxWidth: "100%", background: "#fff", alignSelf: "flex-start" }} />}
        </>
      )}
    </Modal>
  );
}

// ----------------------------------------------------------------- forms

function NewFieldDialog({ page, kind, rect, close }: { page: number; kind: string; rect: Rect; close: () => void }) {
  const [name, setName] = useState(`${kind}_${Math.floor(Math.random() * 9000 + 1000)}`);
  const [options, setOptions] = useState("Option 1\nOption 2\nOption 3");
  const [exportValue, setExportValue] = useState("Choice1");
  const [required, setRequired] = useState(false);
  const [fontSize, setFontSize] = useState(0);
  const choice = kind === "combo" || kind === "list";
  const ok = async () => {
    close();
    await mutate("Adding field", (id) => api.addField(id, {
      page, kind, name: name.trim(), rect, required, font_size: fontSize,
      options: choice ? options.split("\n").map((s) => s.trim()).filter(Boolean) : [],
      export_value: kind === "radio" ? exportValue : "",
    }));
  };
  return (
    <Modal title="New form field" onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" onClick={ok} disabled={!name.trim()}>Add field</button></>}>
      <Row label={kind === "radio" ? "Group name (radios with the same name work together)" : "Field name"}>
        <input type="text" autoFocus value={name} onChange={(e) => setName(e.target.value)} onFocus={(e) => e.target.select()} />
      </Row>
      {kind === "radio" && <Row label="Value when this option is chosen"><input type="text" value={exportValue} onChange={(e) => setExportValue(e.target.value)} /></Row>}
      {choice && <Row label="Options, one per line"><textarea value={options} onChange={(e) => setOptions(e.target.value)} /></Row>}
      {(kind === "text" || kind === "multiline") && (
        <Row label="Font size (0 = auto)"><input type="number" min={0} max={72} value={fontSize} onChange={(e) => setFontSize(+e.target.value)} /></Row>
      )}
      <label className="check"><input type="checkbox" checked={required} onChange={(e) => setRequired(e.target.checked)} /> Required</label>
    </Modal>
  );
}

function FormToolsDialog({ close }: { close: () => void }) {
  const t = activeTab();
  if (!t) return null;
  return (
    <Modal title="Form data" onClose={close} footer={<button className="btn" onClick={close}>Close</button>}>
      <button className="btn" onClick={async () => {
        const p = await pickSavePath(`${stem(t.info.name)} - form data.json`, [{ name: "JSON", extensions: ["json"] }]);
        if (p && (await task("Exporting", () => api.exportFormData(t.info.id, p)).then(() => true))) useStore.getState().toast("Form data exported");
      }}>Export form data (JSON)</button>
      <button className="btn" onClick={async () => {
        const p = await openFileDialog({ filters: [{ name: "JSON", extensions: ["json"] }] });
        if (typeof p !== "string") return;
        let n = 0;
        await mutate("Importing", async (id) => { n = await api.importFormData(id, p); });
        useStore.getState().toast(`Filled ${n} field${n === 1 ? "" : "s"}`);
      }}>Import form data (JSON)</button>
      <button className="btn" onClick={() => { close(); mutate("Clearing form", (id) => api.resetForm(id)); }}>Clear all fields</button>
      <button className="btn danger" onClick={() => { close(); mutate("Flattening", (id) => api.flatten(id)); }}>Flatten form and comments</button>
      <p className="muted" style={{ margin: 0 }}>Flattening turns fields and comments into ordinary page content so they can no longer be changed.</p>
    </Modal>
  );
}

// ----------------------------------------------------------------- page tools

function WatermarkDialog({ close }: { close: () => void }) {
  const [text, setText] = useState("CONFIDENTIAL");
  const [size, setSize] = useState(64);
  const [color, setColor] = useState("#e03131");
  const [opacityLight, setLight] = useState(true);
  const [angle, setAngle] = useState(45);
  const [pages, setPages] = useState("");
  const [behind, setBehind] = useState(true);
  const lighten = (hex: string) => {
    const n = parseInt(hex.slice(1), 16);
    const mix = (c: number) => Math.round(c + (255 - c) * 0.6);
    return `#${[(n >> 16) & 255, (n >> 8) & 255, n & 255].map(mix).map((c) => c.toString(16).padStart(2, "0")).join("")}`;
  };
  const ok = () => { close(); mutate("Adding watermark", (id) => api.watermark(id, { text, size, color: opacityLight ? lighten(color) : color, angle, pages, behind })); };
  return (
    <Modal title="Add watermark" onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" disabled={!text.trim()} onClick={ok}>Add watermark</button></>}>
      <Row label="Text"><input type="text" value={text} onChange={(e) => setText(e.target.value)} autoFocus /></Row>
      <div className="grid3">
        <Row label="Size (pt)"><input type="number" value={size} onChange={(e) => setSize(+e.target.value)} /></Row>
        <Row label="Angle (°)"><input type="number" value={angle} onChange={(e) => setAngle(+e.target.value)} /></Row>
        <Row label="Colour"><input type="color" value={color} onChange={(e) => setColor(e.target.value)} style={{ height: 32, width: "100%" }} /></Row>
      </div>
      <Row label="Pages (blank = all, e.g. 1-3, 7)"><input type="text" value={pages} onChange={(e) => setPages(e.target.value)} /></Row>
      <label className="check"><input type="checkbox" checked={opacityLight} onChange={(e) => setLight(e.target.checked)} /> Light tint</label>
      <label className="check"><input type="checkbox" checked={behind} onChange={(e) => setBehind(e.target.checked)} /> Place behind page content</label>
    </Modal>
  );
}

function HeaderFooterDialog({ close }: { close: () => void }) {
  const t = activeTab();
  const [v, setV] = useState({ top_left: "", top_center: "", top_right: "", bottom_left: "", bottom_center: "Page {page} of {total}", bottom_right: "" });
  const [size, setSize] = useState(9);
  const [margin, setMargin] = useState(28);
  const [start, setStart] = useState(1);
  const [pages, setPages] = useState("");
  const [color, setColor] = useState("#444444");
  if (!t) return null;
  const slot = (k: keyof typeof v, label: string) => <Row label={label}><input type="text" value={v[k]} onChange={(e) => setV({ ...v, [k]: e.target.value })} /></Row>;
  const ok = () => {
    close();
    const date = new Date().toLocaleDateString();
    mutate("Adding header & footer", (id) => api.headerFooter(id, { ...v, size, color, margin, start_number: start, pages, date }, stem(t.info.name)));
  };
  return (
    <Modal wide title="Header, footer & page numbers" onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" onClick={ok}>Add</button></>}>
      <p className="muted" style={{ margin: 0 }}>Use {"{page}"}, {"{total}"}, {"{date}"} and {"{name}"} in any slot. For Bates numbering, put a prefix before {"{page}"} and set the start number.</p>
      <div className="grid3">{slot("top_left", "Header left")}{slot("top_center", "Header centre")}{slot("top_right", "Header right")}</div>
      <div className="grid3">{slot("bottom_left", "Footer left")}{slot("bottom_center", "Footer centre")}{slot("bottom_right", "Footer right")}</div>
      <div className="grid3">
        <Row label="Font size"><input type="number" value={size} onChange={(e) => setSize(+e.target.value)} /></Row>
        <Row label="Margin (pt)"><input type="number" value={margin} onChange={(e) => setMargin(+e.target.value)} /></Row>
        <Row label="Start number"><input type="number" value={start} onChange={(e) => setStart(+e.target.value)} /></Row>
      </div>
      <div className="grid2">
        <Row label="Pages (blank = all)"><input type="text" value={pages} onChange={(e) => setPages(e.target.value)} /></Row>
        <Row label="Colour"><input type="color" value={color} onChange={(e) => setColor(e.target.value)} style={{ height: 32, width: "100%" }} /></Row>
      </div>
    </Modal>
  );
}

function CombineDialog({ close }: { close: () => void }) {
  const [files, setFiles] = useState<string[]>(() => {
    const t = activeTab();
    return t?.info.path ? [t.info.path] : [];
  });
  const add = async () => {
    const p = await openFileDialog({ multiple: true, filters: [{ name: "PDFs and images", extensions: ["pdf", "png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp"] }] });
    if (p) setFiles([...files, ...(Array.isArray(p) ? p : [p])]);
  };
  const move = (i: number, d: number) => {
    const next = [...files];
    const [x] = next.splice(i, 1);
    next.splice(i + d, 0, x);
    setFiles(next);
  };
  const ok = async () => {
    close();
    const info = await task("Combining", () => api.combine(files));
    if (info) useStore.getState().addTab(info);
  };
  return (
    <Modal wide title="Combine files" onClose={close} footer={<><button className="btn" onClick={add}>Add files…</button><span className="grow" /><button className="btn" onClick={close}>Cancel</button><button className="btn primary" disabled={files.length < 1} onClick={ok}>Combine</button></>}>
      <p className="muted" style={{ margin: 0 }}>Files are joined in this order into a new document. Images become one page each.</p>
      <div className="file-list">
        {files.length === 0 && <div className="empty">Add PDFs or images to combine.</div>}
        {files.map((f, i) => (
          <div className="row" key={`${f}:${i}`}>
            <span className="faint" style={{ width: 22 }}>{i + 1}</span>
            <span className="grow" title={f}>{baseName(f)}</span>
            <button className="ibtn small" disabled={i === 0} onClick={() => move(i, -1)} aria-label="Move up"><ArrowUp size={14} /></button>
            <button className="ibtn small" disabled={i === files.length - 1} onClick={() => move(i, 1)} aria-label="Move down"><ArrowDown size={14} /></button>
            <button className="ibtn small" onClick={() => setFiles(files.filter((_, j) => j !== i))} aria-label="Remove"><X size={14} /></button>
          </div>
        ))}
      </div>
    </Modal>
  );
}

function SplitDialog({ close }: { close: () => void }) {
  const t = activeTab();
  const [mode, setMode] = useState<"every" | "ranges" | "size">("every");
  const [every, setEvery] = useState(1);
  const [ranges, setRanges] = useState("1-2; 3-");
  const [mb, setMb] = useState(5);
  if (!t) return null;
  const ok = async () => {
    const dir = await pickFolder();
    if (!dir) return;
    close();
    const out = await task("Splitting", () => api.split(t.info.id, mode === "every" ? { every } : mode === "ranges" ? { ranges } : { maxMb: mb }, dir, stem(t.info.name)));
    if (out) useStore.getState().toast(`Created ${out.length} file${out.length > 1 ? "s" : ""}`, "info", { label: "Show", run: () => api.reveal(out[0]) });
  };
  return (
    <Modal title="Split document" onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" onClick={ok}>Choose folder and split</button></>}>
      <div className="seg">
        <button className={mode === "every" ? "on" : ""} onClick={() => setMode("every")}>By page count</button>
        <button className={mode === "ranges" ? "on" : ""} onClick={() => setMode("ranges")}>By ranges</button>
        <button className={mode === "size" ? "on" : ""} onClick={() => setMode("size")}>By file size</button>
      </div>
      {mode === "every" && <Row label="Pages per file"><input type="number" min={1} value={every} onChange={(e) => setEvery(Math.max(1, +e.target.value))} /></Row>}
      {mode === "ranges" && <Row label="Ranges, separated by semicolons (each becomes a file)"><input type="text" value={ranges} onChange={(e) => setRanges(e.target.value)} /></Row>}
      {mode === "size" && <Row label="Largest file size (MB)"><input type="number" min={0.1} step={0.5} value={mb} onChange={(e) => setMb(+e.target.value)} /></Row>}
      <p className="muted" style={{ margin: 0 }}>{t.info.page_count} pages. The open document is not changed.</p>
    </Modal>
  );
}

// ----------------------------------------------------------------- protect

function ProtectDialog({ close }: { close: () => void }) {
  const t = activeTab();
  const [openPw, setOpenPw] = useState("");
  const [confirm, setConfirm] = useState("");
  const [restrict, setRestrict] = useState(false);
  const [ownerPw, setOwnerPw] = useState("");
  const [perm, setPerm] = useState({ allow_print: true, allow_copy: false, allow_modify: false, allow_annotate: true, allow_forms: true });
  if (!t) return null;
  const mismatch = openPw !== confirm;
  const valid = (openPw || (restrict && ownerPw)) && !mismatch;
  const apply = async () => {
    const sec: Security = {
      user_password: openPw,
      owner_password: restrict ? ownerPw : openPw,
      ...(restrict ? perm : { allow_print: true, allow_copy: true, allow_modify: true, allow_annotate: true, allow_forms: true }),
    };
    close();
    if (await mutate("Applying protection", (id) => api.setSecurity(id, sec))) useStore.getState().toast("Protection is applied when you save");
  };
  const box = (k: keyof typeof perm, label: string) => <label className="check"><input type="checkbox" checked={perm[k]} onChange={(e) => setPerm({ ...perm, [k]: e.target.checked })} /> {label}</label>;
  return (
    <Modal title="Password protect" onClose={close} footer={<>
      {t.info.protected && <button className="btn" onClick={() => { close(); mutate("Removing protection", (id) => api.setSecurity(id, null)).then((ok) => ok && useStore.getState().toast("Protection is removed when you save")); }}>Remove protection</button>}
      <span className="grow" />
      <button className="btn" onClick={close}>Cancel</button>
      <button className="btn primary" disabled={!valid} onClick={apply}>Apply</button>
    </>}>
      <p className="muted" style={{ margin: 0 }}>Uses AES-256 encryption.{t.info.protected ? " This document is already protected; new settings replace the old ones." : ""}</p>
      <div className="grid2">
        <Row label="Password to open (optional)"><input type="password" value={openPw} onChange={(e) => setOpenPw(e.target.value)} autoFocus /></Row>
        <Row label="Confirm password"><input type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} /></Row>
      </div>
      {mismatch && confirm && <span style={{ color: "var(--danger)" }}>The passwords do not match.</span>}
      <label className="check"><input type="checkbox" checked={restrict} onChange={(e) => setRestrict(e.target.checked)} /> Restrict editing and printing</label>
      {restrict && (
        <>
          <Row label="Permissions password (needed to change restrictions)"><input type="password" value={ownerPw} onChange={(e) => setOwnerPw(e.target.value)} /></Row>
          <div className="grid2">
            {box("allow_print", "Allow printing")}
            {box("allow_copy", "Allow copying text and images")}
            {box("allow_modify", "Allow editing pages")}
            {box("allow_annotate", "Allow comments")}
            {box("allow_forms", "Allow filling forms")}
          </div>
        </>
      )}
    </Modal>
  );
}

function SanitizeDialog({ close }: { close: () => void }) {
  const [o, setO] = useState({ metadata: true, attachments: true, javascript: true, links: false });
  const box = (k: keyof typeof o, label: string) => <label className="check"><input type="checkbox" checked={o[k]} onChange={(e) => setO({ ...o, [k]: e.target.checked })} /> {label}</label>;
  return (
    <Modal title="Remove hidden information" onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" onClick={() => { close(); mutate("Removing", (id) => api.sanitize(id, o)); }}>Remove</button></>}>
      {box("metadata", "Document properties and metadata (title, author, XMP)")}
      {box("attachments", "Attached files")}
      {box("javascript", "Scripts and automatic actions")}
      {box("links", "Links")}
      <p className="muted" style={{ margin: 0 }}>To remove visible text or images, use Redact.</p>
    </Modal>
  );
}

function RedactSearchDialog({ close }: { close: () => void }) {
  const t = activeTab();
  const patchTab = useStore((s) => s.patchTab);
  const [q, setQ] = useState("");
  const [patterns, setPatterns] = useState({ email: false, phone: false });
  if (!t) return null;
  const run = async () => {
    const queries = [q.trim()].filter(Boolean);
    const areas: { page: number; rect: Rect }[] = [];
    await task("Searching", async () => {
      for (const query of queries) {
        for (const h of await api.search(t.info.id, query, false, false)) h.rects.forEach((r) => areas.push({ page: h.page, rect: r }));
      }
      if (patterns.email || patterns.phone) {
        const re = patterns.email && patterns.phone ? /([\w.+-]+@[\w-]+\.[\w.]+|\+?\d[\d\s()-]{7,}\d)/g : patterns.email ? /[\w.+-]+@[\w-]+\.[\w.]+/g : /\+?\d[\d\s()-]{7,}\d/g;
        for (const p of t.info.pages) {
          const chars = await api.pageChars(t.info.id, p.index);
          const text = chars.map((c) => c.c).join("");
          // map string offsets back to chars (c strings may be multi-unit)
          const offsets: number[] = [];
          chars.forEach((c, i) => { for (let k = 0; k < c.c.length; k++) offsets.push(i); });
          for (const m of text.matchAll(re)) {
            const a = offsets[m.index!];
            const b = offsets[m.index! + m[0].length - 1];
            lineRects(chars.slice(a, b + 1)).forEach((r) => areas.push({ page: p.index, rect: r }));
          }
        }
      }
    });
    patchTab(t.info.id, { redactions: [...t.redactions, ...areas] });
    useStore.getState().setTool("redact");
    useStore.getState().toast(areas.length ? `Marked ${areas.length} area${areas.length > 1 ? "s" : ""}. Review, then Apply redactions.` : "Nothing found to mark.");
    close();
  };
  return (
    <Modal title="Find text to redact" onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" disabled={!q.trim() && !patterns.email && !patterns.phone} onClick={run}>Mark matches</button></>}>
      <Row label="Word or phrase"><input type="text" value={q} onChange={(e) => setQ(e.target.value)} autoFocus /></Row>
      <label className="check"><input type="checkbox" checked={patterns.email} onChange={(e) => setPatterns({ ...patterns, email: e.target.checked })} /> All email addresses</label>
      <label className="check"><input type="checkbox" checked={patterns.phone} onChange={(e) => setPatterns({ ...patterns, phone: e.target.checked })} /> All phone numbers</label>
    </Modal>
  );
}

// ----------------------------------------------------------------- convert

function ExportDialog({ close }: { close: () => void }) {
  const t = activeTab();
  const [fmt, setFmt] = useState<"docx" | "xlsx" | "png" | "jpg" | "txt" | "pdf">("docx");
  const [dpi, setDpi] = useState(150);
  const [images, setImages] = useState(true);
  if (!t) return null;
  const s = stem(t.info.name);
  const done = (path: string) => useStore.getState().toast(`Exported ${baseName(path)}`, "info", { label: "Open", run: () => api.openExternally(path) });
  const run = async () => {
    const id = t.info.id;
    if (fmt === "docx") {
      const p = await pickSavePath(`${s}.docx`, [{ name: "Word document", extensions: ["docx"] }]);
      if (p && (await task("Exporting to Word", () => api.exportDocx(id, p, images).then(() => true)))) done(p);
    } else if (fmt === "xlsx") {
      const p = await pickSavePath(`${s}.xlsx`, [{ name: "Excel workbook", extensions: ["xlsx"] }]);
      if (p && (await task("Exporting to Excel", () => api.exportXlsx(id, p).then(() => true)))) done(p);
    } else if (fmt === "txt") {
      const p = await pickSavePath(`${s}.txt`, [{ name: "Text", extensions: ["txt"] }]);
      if (p && (await task("Exporting text", () => api.exportText(id, p).then(() => true)))) done(p);
    } else if (fmt === "pdf") {
      const p = await pickSavePath(`${s} copy.pdf`, PDF_FILTER);
      if (p && (await task("Saving copy", () => api.saveCopy(id, p).then(() => true)))) done(p);
    } else {
      const dir = await pickFolder();
      if (!dir) return;
      const out = await task("Exporting images", () => api.exportImages(id, t.info.pages.map((x) => x.index), dpi, fmt, dir, s));
      if (out) useStore.getState().toast(`Exported ${out.length} image${out.length > 1 ? "s" : ""}`, "info", { label: "Show", run: () => api.reveal(out[0]) });
    }
    close();
  };
  const opt = (k: typeof fmt, label: string) => <button className={fmt === k ? "on" : ""} onClick={() => setFmt(k)}>{label}</button>;
  return (
    <Modal title="Export" onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" onClick={run}>Export</button></>}>
      <div className="seg" style={{ flexWrap: "wrap" }}>{opt("docx", "Word")}{opt("xlsx", "Excel")}{opt("png", "PNG")}{opt("jpg", "JPEG")}{opt("txt", "Text")}{opt("pdf", "PDF copy")}</div>
      {fmt === "docx" && <label className="check"><input type="checkbox" checked={images} onChange={(e) => setImages(e.target.checked)} /> Include images</label>}
      {(fmt === "png" || fmt === "jpg") && <Row label="Resolution (dpi)"><select value={dpi} onChange={(e) => setDpi(+e.target.value)}>{[72, 96, 150, 200, 300, 600].map((d) => <option key={d} value={d}>{d}</option>)}</select></Row>}
      <p className="muted" style={{ margin: 0 }}>
        {fmt === "docx" && "Text is rebuilt into editable paragraphs. Complex layouts may need touching up."}
        {fmt === "xlsx" && "Each page becomes a sheet; aligned text becomes rows and columns."}
        {(fmt === "png" || fmt === "jpg") && "One image per page, saved to a folder you choose."}
        {fmt === "txt" && "Plain text in reading order."}
        {fmt === "pdf" && "Saves a copy with your changes; the open document keeps its location."}
      </p>
    </Modal>
  );
}

function OcrDialog({ close }: { close: () => void }) {
  const t = activeTab();
  const [langs, setLangs] = useState({ eng: true, ben: false });
  const [which, setWhich] = useState<"scanned" | "all" | "current">("scanned");
  const [progress, setProgress] = useState<{ page: number; done: number; total: number; p: number } | null>(null);
  if (!t) return null;
  const run = async () => {
    const id = t.info.id;
    const l = Object.entries(langs).filter(([, v]) => v).map(([k]) => k).join("+") || "eng";
    let pages = which === "current" ? [t.page] : t.info.pages.map((p) => p.index);
    try {
      if (which === "scanned") {
        const keep: number[] = [];
        for (const p of pages) if (!(await api.pageHasText(id, p))) keep.push(p);
        pages = keep;
      }
      if (!pages.length) {
        useStore.getState().toast("Every page already has text. Choose All pages to OCR anyway.");
        close();
        return;
      }
      setProgress({ page: pages[0], done: 0, total: pages.length, p: 0 });
      const words = await ocrPages(id, pages, l, (page, done, p) => setProgress({ page, done, total: pages.length, p }));
      const info = await api.info(id);
      useStore.getState().updateInfo(id, info);
      useStore.getState().toast(`Recognised ${words} words on ${pages.length} page${pages.length > 1 ? "s" : ""}. The text is now searchable.`);
      close();
    } catch (e) {
      useStore.getState().toast(errMsg(e), "error");
      setProgress(null);
    }
  };
  const pct = progress ? ((progress.done + progress.p) / progress.total) * 100 : 0;
  return (
    <Modal title="Recognise text (OCR)" onClose={() => !progress && close()} footer={<><button className="btn" disabled={!!progress} onClick={close}>Cancel</button><button className="btn primary" disabled={!!progress} onClick={run}>Recognise text</button></>}>
      <p className="muted" style={{ margin: 0 }}>Adds an invisible text layer to scanned pages so you can search, select and copy their text. Runs on this computer.</p>
      <div className="seg">
        <button className={which === "scanned" ? "on" : ""} onClick={() => setWhich("scanned")}>Pages without text</button>
        <button className={which === "all" ? "on" : ""} onClick={() => setWhich("all")}>All pages</button>
        <button className={which === "current" ? "on" : ""} onClick={() => setWhich("current")}>Current page</button>
      </div>
      <div style={{ display: "flex", gap: 16 }}>
        <label className="check"><input type="checkbox" checked={langs.eng} onChange={(e) => setLangs({ ...langs, eng: e.target.checked })} /> English</label>
        <label className="check"><input type="checkbox" checked={langs.ben} onChange={(e) => setLangs({ ...langs, ben: e.target.checked })} /> Bangla</label>
      </div>
      {progress && (
        <>
          <div className="progress"><i style={{ width: `${pct}%` }} /></div>
          <span className="muted">Page {progress.page + 1} ({progress.done} of {progress.total} done)</span>
        </>
      )}
    </Modal>
  );
}

function CompressDialog({ close }: { close: () => void }) {
  const [level, setLevel] = useState<"low" | "medium" | "high">("medium");
  const [result, setResult] = useState<{ before: number; after: number; images_recompressed: number } | null>(null);
  const mb = (n: number) => `${(n / 1024 / 1024).toFixed(2)} MB`;
  const run = async () => {
    let r: typeof result = null;
    await mutate("Reducing file size", async (id) => { r = await api.compress(id, level); });
    setResult(r);
  };
  return (
    <Modal title="Reduce file size" onClose={close} footer={result ? <button className="btn primary" onClick={close}>Done</button> : <><button className="btn" onClick={close}>Cancel</button><button className="btn primary" onClick={run}>Reduce</button></>}>
      {!result ? (
        <>
          <div className="seg">
            <button className={level === "low" ? "on" : ""} onClick={() => setLevel("low")}>Best quality</button>
            <button className={level === "medium" ? "on" : ""} onClick={() => setLevel("medium")}>Balanced</button>
            <button className={level === "high" ? "on" : ""} onClick={() => setLevel("high")}>Smallest file</button>
          </div>
          <p className="muted" style={{ margin: 0 }}>Large images are downsampled and recompressed, and unused data is removed.</p>
        </>
      ) : result.after < result.before ? (
        <p style={{ margin: 0 }}>Reduced from {mb(result.before)} to {mb(result.after)} ({Math.round((1 - result.after / result.before) * 100)}% smaller, {result.images_recompressed} images recompressed). Save to keep the smaller file.</p>
      ) : (
        <p style={{ margin: 0 }}>This document is already compact ({mb(result.before)}). Nothing was changed.</p>
      )}
    </Modal>
  );
}

function PropertiesDialog({ close }: { close: () => void }) {
  const t = activeTab();
  const [m, setM] = useState(() => ({ title: t?.info.title ?? "", author: t?.info.author ?? "", subject: t?.info.subject ?? "", keywords: t?.info.keywords ?? "", creator: t?.info.creator ?? "" }));
  if (!t) return null;
  const i = t.info;
  const dirty = m.title !== i.title || m.author !== i.author || m.subject !== i.subject || m.keywords !== i.keywords || m.creator !== i.creator;
  const pdfDate = (s: string) => {
    const r = /D:(\d{4})(\d{2})?(\d{2})?(\d{2})?(\d{2})?/.exec(s);
    return r ? new Date(+r[1], +(r[2] ?? 1) - 1, +(r[3] ?? 1), +(r[4] ?? 0), +(r[5] ?? 0)).toLocaleString() : s || "—";
  };
  const f = (k: keyof typeof m, label: string) => <Row label={label}><input type="text" value={m[k]} onChange={(e) => setM({ ...m, [k]: e.target.value })} /></Row>;
  return (
    <Modal wide title="Document properties" onClose={close} footer={<><button className="btn" onClick={close}>Cancel</button><button className="btn primary" disabled={!dirty} onClick={() => { close(); mutate("Updating properties", (id) => api.setMetadata(id, m)); }}>Save properties</button></>}>
      <div className="grid2">{f("title", "Title")}{f("author", "Author")}</div>
      <div className="grid2">{f("subject", "Subject")}{f("keywords", "Keywords")}</div>
      {f("creator", "Application")}
      <div className="grid2 muted">
        <div>File: {i.path ? <a href="#" onClick={(e) => { e.preventDefault(); api.reveal(i.path!); }}>{baseName(i.path)}</a> : "not saved"}</div>
        <div>Folder: {i.path ? dirName(i.path) : "—"}</div>
        <div>Pages: {i.page_count}</div>
        <div>PDF version: {i.version.replace("Pdf", "").replace(/_/g, ".")}</div>
        <div>Created: {pdfDate(i.created)}</div>
        <div>Modified: {pdfDate(i.modified)}</div>
        <div>Producer: {i.producer || "—"}</div>
        <div>Security: {i.protected ? "Password protected" : "None"}</div>
      </div>
      <AuthorField />
    </Modal>
  );
}

function AuthorField() {
  const author = useStore((s) => s.author);
  const setAuthor = useStore((s) => s.setAuthor);
  return (
    <Row label="Your name on comments">
      <input type="text" value={author} onChange={(e) => { setAuthor(e.target.value); try { localStorage.setItem("folio.author", e.target.value); } catch { /* ignore */ } }} />
    </Row>
  );
}
