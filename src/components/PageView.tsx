import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Trash2, ImageDown, Replace } from "lucide-react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useStore, type DocTab, type Tool } from "../store";
import { api, type AnnotInfo, type FieldInfo, type ObjInfo, type PageInfo, type Rect, type TextChar, type NewAnnot } from "../api";
import { requestRender } from "../render";
import { mutate, IMAGE_FILTER, pickSavePath, task } from "../actions";
import { STAMPS } from "./OptionsBar";

type Pt = { x: number; y: number };
const MARKUP_TOOLS: Tool[] = ["highlight", "underline", "strikeout", "squiggly"];
const DRAG_TOOLS: Tool[] = ["rect", "ellipse", "line", "arrow", "textbox", "stamp", "sign", "addimage", "field", "redact", "crop"];

function normRect(a: Pt, b: Pt): Rect {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y) };
}

/** Merge a run of characters into one rectangle per visual line. */
export function lineRects(chars: TextChar[]): Rect[] {
  const out: Rect[] = [];
  let cur: Rect | null = null;
  for (const c of chars) {
    if (c.r.w <= 0 || c.r.h <= 0) continue;
    if (cur) {
      const cy = c.r.y + c.r.h / 2;
      const sameLine = cy > cur.y && cy < cur.y + cur.h && c.r.x >= cur.x - 2;
      if (sameLine) {
        const x2 = Math.max(cur.x + cur.w, c.r.x + c.r.w);
        const y1 = Math.min(cur.y, c.r.y);
        const y2 = Math.max(cur.y + cur.h, c.r.y + c.r.h);
        cur = { x: Math.min(cur.x, c.r.x), y: y1, w: x2 - Math.min(cur.x, c.r.x), h: y2 - y1 };
        continue;
      }
      out.push(cur);
    }
    cur = { ...c.r };
  }
  if (cur) out.push(cur);
  return out;
}

function charsText(chars: TextChar[]) {
  return chars.map((c) => (c.c === "\r\n" || c.c === "\r" ? "\n" : c.c)).join("").replace(/\n{3,}/g, "\n\n");
}

function nearestChar(chars: TextChar[], p: Pt): number {
  let best = -1;
  let bestD = Infinity;
  for (let i = 0; i < chars.length; i++) {
    const r = chars[i].r;
    if (r.w <= 0 || r.h <= 0) continue;
    if (p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h) return i;
    const dx = p.x < r.x ? r.x - p.x : p.x > r.x + r.w ? p.x - r.x - r.w : 0;
    const dy = p.y < r.y ? r.y - p.y : p.y > r.y + r.h ? p.y - r.y - r.h : 0;
    const d = dx + dy * 4; // prefer the same line
    if (d < bestD) { bestD = d; best = i; }
  }
  return bestD < 60 ? best : -1;
}

function dataUrlToB64(url: string) {
  return url.slice(url.indexOf(",") + 1);
}

async function imageSize(url: string): Promise<{ w: number; h: number }> {
  const img = new Image();
  img.src = url;
  await img.decode().catch(() => undefined);
  return { w: img.naturalWidth || 300, h: img.naturalHeight || 100 };
}

export function PageView({ tab, page, z, visible }: { tab: DocTab; page: PageInfo; z: number; visible: boolean }) {
  const id = tab.info.id;
  const pi = page.index;
  const version = tab.version;
  const tool = useStore((s) => s.tool);
  const opts = useStore((s) => s.opts);
  const author = useStore((s) => s.author);
  const selAnnot = useStore((s) => s.selAnnot);
  const setSelAnnot = useStore((s) => s.setSelAnnot);
  const patchTab = useStore((s) => s.patchTab);
  const openDialog = useStore((s) => s.openDialog);
  const ref = useRef<HTMLDivElement>(null);

  const W = page.width * z;
  const H = page.height * z;
  const dpr = window.devicePixelRatio || 1;
  const scale = Math.min(8, Math.max(0.25, Math.round(z * dpr * 4) / 4));

  // ---------- bitmap
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!visible) return;
    let live = true;
    requestRender(id, version, pi, scale).then((u) => live && setUrl(u)).catch(() => undefined);
    return () => { live = false; };
  }, [visible, id, version, pi, scale]);

  // ---------- per-page data, fetched lazily when visible and needed
  const needChars = visible && (tool === "select" || MARKUP_TOOLS.includes(tool));
  const [chars, setChars] = useState<TextChar[] | null>(null);
  useEffect(() => {
    if (!needChars) return;
    let live = true;
    api.pageChars(id, pi).then((c) => live && setChars(c)).catch(() => live && setChars([]));
    return () => { live = false; };
  }, [needChars, id, pi, version]);

  const [annots, setAnnots] = useState<AnnotInfo[]>([]);
  useEffect(() => {
    if (!visible || tool !== "select") return;
    let live = true;
    api.listAnnots(id, pi).then((a) => live && setAnnots(a.filter((x) => x.kind !== "link"))).catch(() => undefined);
    return () => { live = false; };
  }, [visible, tool, id, pi, version]);

  const [fields, setFields] = useState<FieldInfo[]>([]);
  useEffect(() => {
    if (!visible || !(tool === "select" || tool === "field")) return;
    let live = true;
    api.listFields(id, pi).then((f) => live && setFields(f)).catch(() => undefined);
    return () => { live = false; };
  }, [visible, tool, id, pi, version]);

  const [objs, setObjs] = useState<ObjInfo[]>([]);
  useEffect(() => {
    if (!visible || tool !== "edit") return;
    let live = true;
    api.listObjects(id, pi, false).then((o) => live && setObjs(o)).catch(() => undefined);
    return () => { live = false; };
  }, [visible, tool, id, pi, version]);

  // ---------- interaction state
  const [sel, setSel] = useState<{ a: number; b: number } | null>(null);
  const [draft, setDraft] = useState<{ start: Pt; end: Pt } | null>(null);
  const [stroke, setStroke] = useState<Pt[] | null>(null);
  const [inline, setInline] = useState<{ kind: "textbox" | "addtext" | "edittext"; rect: Rect; value: string; obj?: number; size: number } | null>(null);
  const [selObj, setSelObj] = useState<number | null>(null);
  const [objDrag, setObjDrag] = useState<{ start: Pt; dx: number; dy: number; resize: boolean } | null>(null);
  const [selField, setSelField] = useState<number | null>(null);
  const pointer = useRef<{ mode: string; start: Pt } | null>(null);

  useEffect(() => { setSel(null); setSelObj(null); setSelField(null); setDraft(null); setInline(null); }, [tool]);

  const toView = (e: { clientX: number; clientY: number }): Pt => {
    const r = ref.current!.getBoundingClientRect();
    return { x: (e.clientX - r.left) / z, y: (e.clientY - r.top) / z };
  };

  const selChars = useMemo(() => {
    if (!sel || !chars) return [];
    const [a, b] = sel.a <= sel.b ? [sel.a, sel.b] : [sel.b, sel.a];
    return chars.slice(a, b + 1);
  }, [sel, chars]);
  const selRects = useMemo(() => lineRects(selChars), [selChars]);

  // Copy selected text.
  useEffect(() => {
    if (!selChars.length) return;
    const onCopy = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c") {
        navigator.clipboard.writeText(charsText(selChars)).catch(() => undefined);
      }
    };
    window.addEventListener("keydown", onCopy);
    return () => window.removeEventListener("keydown", onCopy);
  }, [selChars]);

  const add = useCallback((spec: NewAnnot, label = "Adding comment") => mutate(label, (docId) => api.addAnnot(docId, pi, spec, author)), [pi, author]);

  // Delete key for selected objects / fields / annotations on this page.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tgt = e.target as HTMLElement;
      if (tgt.tagName === "INPUT" || tgt.tagName === "TEXTAREA" || tgt.tagName === "SELECT") return;
      if (e.key !== "Delete" && e.key !== "Backspace") return;
      if (tool === "edit" && selObj != null) {
        const o = selObj;
        setSelObj(null);
        mutate("Deleting", (docId) => api.deleteObjects(docId, pi, [o]));
      } else if (tool === "field" && selField != null) {
        const f = selField;
        setSelField(null);
        mutate("Deleting field", (docId) => api.deleteField(docId, pi, f));
      } else if (tool === "select" && selAnnot && selAnnot.id === id && selAnnot.page === pi) {
        const a = selAnnot.index;
        setSelAnnot(null);
        mutate("Deleting comment", (docId) => api.deleteAnnot(docId, pi, a));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [tool, selObj, selField, selAnnot, id, pi, setSelAnnot]);

  // ---------- pointer handling
  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || tool === "hand") return;
    const p = toView(e);
    const target = e.target as HTMLElement;
    if (target.closest(".note-pop, .inline-edit, .form-input, .form-check, select, .obj-box, .annot-hit, .field-design, .redact-mark")) return;
    if (inline) return;
    setSelAnnot(null);
    setSelObj(null);
    setSelField(null);

    if (tool === "select" || (MARKUP_TOOLS.includes(tool) && chars && chars.length)) {
      const i = chars ? nearestChar(chars, p) : -1;
      if (i >= 0) {
        setSel({ a: i, b: i });
        pointer.current = { mode: "text", start: p };
        ref.current!.setPointerCapture(e.pointerId);
        e.preventDefault();
      } else {
        setSel(null);
        if (MARKUP_TOOLS.includes(tool)) {
          pointer.current = { mode: "rect", start: p };
          setDraft({ start: p, end: p });
          ref.current!.setPointerCapture(e.pointerId);
        }
      }
      return;
    }
    if (MARKUP_TOOLS.includes(tool) || DRAG_TOOLS.includes(tool)) {
      pointer.current = { mode: "rect", start: p };
      setDraft({ start: p, end: p });
      ref.current!.setPointerCapture(e.pointerId);
      e.preventDefault();
      return;
    }
    if (tool === "ink") {
      pointer.current = { mode: "ink", start: p };
      setStroke([p]);
      ref.current!.setPointerCapture(e.pointerId);
      e.preventDefault();
      return;
    }
    if (tool === "note") {
      createNote(p);
      return;
    }
    if (tool === "addtext") {
      setInline({ kind: "addtext", rect: { x: p.x, y: p.y - opts.fontSize * 0.5, w: 220, h: opts.fontSize * 1.6 }, value: "", size: opts.fontSize });
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const pt = pointer.current;
    if (!pt) return;
    const p = toView(e);
    if (pt.mode === "text" && chars) {
      const i = nearestChar(chars, p);
      if (i >= 0) setSel((s) => (s ? { a: s.a, b: i } : s));
    } else if (pt.mode === "rect") {
      setDraft({ start: pt.start, end: p });
    } else if (pt.mode === "ink") {
      setStroke((s) => (s ? [...s, p] : s));
    } else if (pt.mode === "obj" && objDrag) {
      setObjDrag({ ...objDrag, dx: p.x - objDrag.start.x, dy: p.y - objDrag.start.y });
    }
  };

  const onPointerUp = async (e: React.PointerEvent) => {
    const pt = pointer.current;
    pointer.current = null;
    if (!pt) return;
    const p = toView(e);
    if (pt.mode === "obj") {
      finishObjDrag();
      return;
    }
    if (pt.mode === "text") {
      if (MARKUP_TOOLS.includes(tool) && selRects.length) {
        const rects = selRects.map((r) => ({ x: r.x - 0.5, y: r.y - 0.5, w: r.w + 1, h: r.h + 1 }));
        setSel(null);
        await add({ type: "markup", style: tool as "highlight", rects, color: opts.markupColor });
      }
      return;
    }
    if (pt.mode === "ink") {
      const s = stroke ?? [];
      setStroke(null);
      if (s.length) await add({ type: "ink", strokes: [s.map((q) => [q.x, q.y] as [number, number])], color: opts.color, width: opts.width }, "Drawing");
      return;
    }
    if (pt.mode !== "rect") return;
    setDraft(null);
    const r = normRect(pt.start, p);
    const click = r.w < 4 && r.h < 4;
    await finishRect(r, click, pt.start, p);
  };

  async function finishRect(r: Rect, click: boolean, a: Pt, b: Pt) {
    switch (tool) {
      case "highlight": case "underline": case "strikeout": case "squiggly":
        if (r.w >= 4 && r.h >= 4) await add({ type: "markup", style: tool, rects: [r], color: opts.markupColor });
        break;
      case "rect": case "ellipse":
        if (!click) await add({ type: "shape", shape: tool, rect: r, color: opts.color, width: opts.width, fill: null }, "Drawing");
        break;
      case "line": case "arrow":
        if (!click) await add({ type: "line", from: [a.x, a.y], to: [b.x, b.y], color: opts.color, width: opts.width, arrow: tool === "arrow" }, "Drawing");
        break;
      case "textbox":
        setInline({ kind: "textbox", rect: click ? { x: r.x, y: r.y, w: 220, h: opts.fontSize * 3 } : r, value: "", size: opts.fontSize });
        break;
      case "stamp": {
        const s = STAMPS.find((x) => x.label === opts.stampLabel) ?? STAMPS[0];
        const w = Math.max(90, s.label.length * 13 + 30);
        const rect = click ? { x: r.x - w / 2, y: r.y - 22, w, h: 44 } : r;
        await add({ type: "stamp", rect, label: s.label, color: s.color }, "Stamping");
        break;
      }
      case "sign": {
        const sig = opts.signature;
        if (!sig) { openDialog("signature", { target: "signature" }); break; }
        const { w, h } = await imageSize(sig);
        const width = click ? 150 : r.w;
        const height = click ? (150 * h) / w : r.h;
        const rect = click ? { x: r.x - width / 2, y: r.y - height / 2, w: width, h: height } : r;
        await add({ type: "image", rect, data: dataUrlToB64(sig) }, "Signing");
        break;
      }
      case "addimage": {
        if (click) break;
        const path = await openFileDialog({ filters: IMAGE_FILTER });
        if (typeof path === "string") await mutate("Adding image", (docId) => api.addImage(docId, pi, r, { path }));
        break;
      }
      case "field": {
        const kind = opts.fieldKind;
        const square = kind === "checkbox" || kind === "radio";
        const rect = click ? (square ? { x: r.x, y: r.y, w: 14, h: 14 } : { x: r.x, y: r.y, w: 180, h: kind === "multiline" ? 60 : kind === "list" ? 70 : 22 }) : r;
        openDialog("new-field", { page: pi, kind, rect });
        break;
      }
      case "redact":
        if (!click) patchTab(id, { redactions: [...(useStore.getState().tabs[id]?.redactions ?? []), { page: pi, rect: r }] });
        break;
      case "crop":
        if (!click) {
          const all = opts.cropAll;
          const pages = all ? tab.info.pages.map((x) => x.index) : [pi];
          openDialog("confirm", {
            title: all ? "Crop all pages?" : `Crop page ${pi + 1}?`,
            body: "Content outside the box is hidden. Use Undo to restore it.",
            ok: "Crop",
            onOk: () => mutate("Cropping", (docId) => api.cropPages(docId, pages, r)),
          });
        }
        break;
    }
  }

  async function createNote(p: Pt) {
    const ok = await add({ type: "note", x: p.x - 10, y: p.y - 10, text: "", color: opts.markupColor }, "Adding note");
    if (!ok) return;
    const list = await api.listAnnots(id, pi).catch(() => []);
    const note = [...list].reverse().find((a) => a.kind === "note");
    if (note) {
      useStore.getState().setTool("select");
      setSelAnnot({ id, page: pi, index: note.index });
    }
  }

  async function commitInline() {
    const it = inline;
    setInline(null);
    if (!it) return;
    const text = it.value.replace(/\s+$/, "");
    if (it.kind === "edittext") {
      const o = objs.find((x) => x.index === it.obj);
      if (o && text !== (o.text ?? "")) await mutate("Editing text", (docId) => api.setObjectText(docId, pi, it.obj!, text));
      return;
    }
    if (!text) return;
    if (it.kind === "textbox") {
      await add({ type: "text_box", rect: it.rect, text, size: it.size, color: opts.color, border: null }, "Adding text box");
    } else {
      await mutate("Adding text", (docId) => api.addText(docId, pi, { x: it.rect.x, y: it.rect.y, text, size: it.size, color: opts.color }));
    }
  }

  // ---------- edit tool: object dragging
  function startObjDrag(e: React.PointerEvent, o: ObjInfo, resize: boolean) {
    e.stopPropagation();
    e.preventDefault();
    if (!resize && e.detail >= 2 && o.kind === "text") {
      setInline({ kind: "edittext", rect: o.rect, value: o.text ?? "", obj: o.index, size: o.font_size || 11 });
      return;
    }
    setSelObj(o.index);
    const p = toView(e);
    pointer.current = { mode: "obj", start: p };
    setObjDrag({ start: p, dx: 0, dy: 0, resize });
    ref.current!.setPointerCapture(e.pointerId);
  }
  async function finishObjDrag() {
    const d = objDrag;
    setObjDrag(null);
    if (!d || selObj == null || (Math.abs(d.dx) < 1 && Math.abs(d.dy) < 1)) return;
    const o = objs.find((x) => x.index === selObj);
    if (!o) return;
    if (d.resize) {
      const rect = { x: o.rect.x, y: o.rect.y, w: Math.max(4, o.rect.w + d.dx), h: Math.max(4, o.rect.h + d.dy) };
      await mutate("Resizing", (docId) => api.resizeObject(docId, pi, o.index, rect));
    } else {
      await mutate("Moving", (docId) => api.moveObject(docId, pi, o.index, d.dx, d.dy));
    }
  }

  // ---------- render
  const hits = tab.hits.filter((h) => h.page === pi);
  const curHit = tab.hits[tab.hitIndex];
  const redactions = tab.redactions.map((r, i) => ({ ...r, i })).filter((r) => r.page === pi);
  const cursor =
    tool === "select" ? "text" : tool === "hand" ? "grab" : tool === "edit" ? "default" : tool === "note" || tool === "addtext" || tool === "stamp" || tool === "sign" ? "copy" : "crosshair";
  const S = (r: Rect) => ({ left: r.x * z, top: r.y * z, width: r.w * z, height: r.h * z });
  const selectedAnnot = selAnnot && selAnnot.id === id && selAnnot.page === pi ? annots.find((a) => a.index === selAnnot.index) : undefined;

  return (
    <div
      ref={ref}
      className="page"
      style={{ width: W, height: H, cursor }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      aria-label={`Page ${pi + 1}`}
    >
      {url ? <img className="bitmap" src={url} alt="" draggable={false} /> : <div className="loading">{pi + 1}</div>}

      {/* search hits */}
      {hits.map((h, i) =>
        h.rects.map((r, k) => <div key={`${i}:${k}`} className={`hit-rect ${curHit === h ? "current" : ""}`} style={S(r)} />),
      )}

      {/* text selection */}
      {selRects.map((r, i) => <div key={i} className="text-sel" style={S(r)} />)}

      {/* annotation hit boxes (select tool) */}
      {tool === "select" &&
        annots.map((a) => (
          <div
            key={a.index}
            className={`annot-hit ${selectedAnnot?.index === a.index ? "on" : ""}`}
            style={S(a.rect)}
            title={a.contents || undefined}
            onPointerDown={(e) => { e.stopPropagation(); setSel(null); setSelAnnot({ id, page: pi, index: a.index }); }}
          />
        ))}
      {tool === "select" && selectedAnnot && (
        <AnnotPopover key={`${selectedAnnot.index}:${version}`} a={selectedAnnot} z={z} pageW={page.width} onClose={() => setSelAnnot(null)} pi={pi} />
      )}

      {/* form filling */}
      {tool === "select" && fields.map((f) => <FieldInput key={`${f.annot_index}:${version}`} f={f} z={z} pi={pi} />)}

      {/* form design */}
      {tool === "field" &&
        fields.map((f) => (
          <div
            key={f.annot_index}
            className={`field-design ${selField === f.annot_index ? "on" : ""}`}
            style={S(f.rect)}
            title={`${f.name} (${f.kind}). Press Delete to remove.`}
            onPointerDown={(e) => { e.stopPropagation(); setSelField(f.annot_index); }}
          >
            {f.name}
          </div>
        ))}

      {/* redaction marks */}
      {redactions.map((r) => (
        <div key={r.i} className="redact-mark" style={S(r.rect)}>
          {tool === "redact" && (
            <button aria-label="Remove mark" onPointerDown={(e) => e.stopPropagation()} onClick={() => patchTab(id, { redactions: tab.redactions.filter((_, j) => j !== r.i) })}>✕</button>
          )}
        </div>
      ))}

      {/* edit tool objects */}
      {tool === "edit" &&
        objs.map((o) => {
          const on = selObj === o.index;
          const d = on && objDrag ? objDrag : null;
          const rect = d ? (d.resize ? { ...o.rect, w: Math.max(4, o.rect.w + d.dx), h: Math.max(4, o.rect.h + d.dy) } : { ...o.rect, x: o.rect.x + d.dx, y: o.rect.y + d.dy }) : o.rect;
          return (
            <div
              key={o.index}
              className={`obj-box ${o.kind === "text" ? "text" : ""} ${on ? "on" : ""}`}
              style={S({ x: rect.x - 1, y: rect.y - 1, w: rect.w + 2, h: rect.h + 2 })}
              title={o.kind === "text" ? `${o.font_name}, ${o.font_size.toFixed(1)}pt` : o.kind}
              onPointerDown={(e) => startObjDrag(e, o, false)}
              onDoubleClick={(e) => {
                e.stopPropagation();
                if (o.kind === "text") setInline({ kind: "edittext", rect: o.rect, value: o.text ?? "", obj: o.index, size: o.font_size || 11 });
              }}
            >
              {on && o.kind !== "text" && <div className="handle" onPointerDown={(e) => startObjDrag(e, o, true)} />}
            </div>
          );
        })}
      {tool === "edit" && selObj != null && !objDrag && (() => {
        const o = objs.find((x) => x.index === selObj);
        if (!o) return null;
        return (
          <div className="note-pop" style={{ left: Math.min(o.rect.x * z, W - 270), top: (o.rect.y + o.rect.h) * z + 6, width: "auto" }} onPointerDown={(e) => e.stopPropagation()}>
            <div className="row">
              {o.kind !== "image" && (
                <input type="color" aria-label="Colour" defaultValue={o.color} onChange={(e) => { const c = e.target.value; mutate("Recolouring", (docId) => api.setObjectColor(docId, pi, o.index, c)); }} />
              )}
              {o.kind === "image" && (
                <>
                  <button className="ibtn" title="Replace image" onClick={async () => { const p = await openFileDialog({ filters: IMAGE_FILTER }); if (typeof p === "string") mutate("Replacing image", (docId) => api.replaceImage(docId, pi, o.index, p)); }}><Replace size={15} /></button>
                  <button className="ibtn" title="Save image as…" onClick={async () => { const p = await pickSavePath("image.png", [{ name: "PNG image", extensions: ["png"] }]); if (p) task("Saving image", () => api.extractImage(id, pi, o.index, p)); }}><ImageDown size={15} /></button>
                </>
              )}
              <button className="ibtn danger" title="Delete (Del)" onClick={() => { setSelObj(null); mutate("Deleting", (docId) => api.deleteObjects(docId, pi, [o.index])); }}><Trash2 size={15} /></button>
            </div>
          </div>
        );
      })()}

      {/* drafts */}
      {draft && (tool === "line" || tool === "arrow") ? (
        <svg className="layer" width={W} height={H} style={{ pointerEvents: "none" }}>
          <line x1={draft.start.x * z} y1={draft.start.y * z} x2={draft.end.x * z} y2={draft.end.y * z} stroke={opts.color} strokeWidth={opts.width * z} strokeLinecap="round" />
        </svg>
      ) : draft && tool === "ellipse" ? (
        <svg className="layer" width={W} height={H} style={{ pointerEvents: "none" }}>
          {(() => { const r = normRect(draft.start, draft.end); return <ellipse cx={(r.x + r.w / 2) * z} cy={(r.y + r.h / 2) * z} rx={(r.w / 2) * z} ry={(r.h / 2) * z} fill="none" stroke={opts.color} strokeWidth={opts.width * z} />; })()}
        </svg>
      ) : draft ? (
        <div className={`draft ${tool === "redact" ? "danger" : ""}`} style={{ ...S(normRect(draft.start, draft.end)), ...(tool === "rect" ? { borderStyle: "solid", borderColor: opts.color, borderWidth: Math.max(1, opts.width * z), background: "none" } : {}), ...(MARKUP_TOOLS.includes(tool) ? { background: `${opts.markupColor}66`, border: "none" } : {}) }} />
      ) : null}
      {stroke && (
        <svg className="layer" width={W} height={H} style={{ pointerEvents: "none" }}>
          <polyline points={stroke.map((q) => `${q.x * z},${q.y * z}`).join(" ")} fill="none" stroke={opts.color} strokeWidth={opts.width * z} strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )}

      {/* inline text entry */}
      {inline && (
        <textarea
          className="inline-edit"
          autoFocus
          style={{
            left: inline.rect.x * z,
            top: inline.rect.y * z,
            width: Math.max(80, inline.rect.w * z + (inline.kind === "edittext" ? 40 : 0)),
            minHeight: Math.max(inline.size * 1.4 * z, inline.rect.h * z),
            fontSize: inline.size * z,
            color: inline.kind === "edittext" ? "#111" : opts.color,
          }}
          value={inline.value}
          placeholder={inline.kind === "edittext" ? "" : "Type here"}
          onPointerDown={(e) => e.stopPropagation()}
          onChange={(e) => setInline({ ...inline, value: e.target.value })}
          onBlur={commitInline}
          onKeyDown={(e) => {
            if (e.key === "Escape") { setInline(null); }
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey || inline.kind === "edittext")) { e.preventDefault(); (e.target as HTMLTextAreaElement).blur(); }
          }}
        />
      )}
    </div>
  );
}

function AnnotPopover({ a, z, pageW, onClose, pi }: { a: AnnotInfo; z: number; pageW: number; onClose: () => void; pi: number }) {
  const [text, setText] = useState(a.contents);
  const save = (u: { contents?: string; color?: string }) => mutate("Updating comment", (docId) => api.updateAnnot(docId, pi, a.index, u));
  const left = Math.max(0, Math.min((a.rect.x) * z, pageW * z - 270));
  return (
    <div className="note-pop" style={{ left, top: (a.rect.y + a.rect.h) * z + 6 }} onPointerDown={(e) => e.stopPropagation()}>
      <div className="row">
        <strong className="grow">{a.author || "Comment"}</strong>
        <span className="faint">{a.kind}</span>
      </div>
      <textarea
        autoFocus={a.kind === "note"}
        placeholder="Add a comment"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text !== a.contents && save({ contents: text })}
      />
      <div className="row">
        <input type="color" aria-label="Colour" defaultValue={a.color || "#ffd43b"} onChange={(e) => save({ color: e.target.value })} />
        <span className="grow" />
        <button className="ibtn danger" title="Delete (Del)" onClick={() => { onClose(); mutate("Deleting comment", (docId) => api.deleteAnnot(docId, pi, a.index)); }}><Trash2 size={15} /></button>
        <button className="btn" onClick={onClose}>Close</button>
      </div>
    </div>
  );
}

function FieldInput({ f, z, pi }: { f: FieldInfo; z: number; pi: number }) {
  const [value, setValue] = useState(f.value);
  const style = { left: f.rect.x * z, top: f.rect.y * z, width: f.rect.w * z, height: f.rect.h * z, fontSize: Math.min(f.rect.h * 0.62, 12) * z };
  const set = (v: Parameters<typeof api.setField>[3]) => mutate("Filling", (docId) => api.setField(docId, pi, f.annot_index, v));
  const stop = (e: React.PointerEvent) => e.stopPropagation();
  if (f.read_only || f.kind === "button") return null;
  if (f.kind === "checkbox" || f.kind === "radio") {
    return (
      <div
        role={f.kind}
        aria-checked={f.checked}
        aria-label={f.name}
        tabIndex={0}
        className={`form-check ${f.kind === "radio" ? "radio" : ""} ${f.checked ? "on" : ""}`}
        style={style}
        onPointerDown={stop}
        onClick={() => set({ kind: "check", checked: f.kind === "radio" ? true : !f.checked })}
        onKeyDown={(e) => { if (e.key === " " || e.key === "Enter") { e.preventDefault(); set({ kind: "check", checked: f.kind === "radio" ? true : !f.checked }); } }}
      >
        <span />
      </div>
    );
  }
  if (f.kind === "combo" || f.kind === "list") {
    return (
      <select className="form-input" aria-label={f.name} style={style} value={value} size={f.kind === "list" ? Math.max(2, Math.floor(f.rect.h / 14)) : undefined} onPointerDown={stop}
        onChange={(e) => { setValue(e.target.value); set({ kind: "choice", value: e.target.value }); }}>
        {f.kind === "combo" && !f.options.includes(value) && <option value={value}>{value}</option>}
        {f.options.map((o) => <option key={o} value={o}>{o}</option>)}
      </select>
    );
  }
  if (f.kind === "signature") {
    return (
      <div className="form-check" style={{ ...style, borderStyle: "dashed", fontSize: 11 * z, color: "#0b6e79" }} onPointerDown={stop} title="Click to sign"
        onClick={async () => {
          const sig = useStore.getState().opts.signature;
          if (!sig) { useStore.getState().openDialog("signature", { target: "signature" }); return; }
          const { w, h } = await imageSize(sig);
          const s = Math.min(f.rect.w / w, f.rect.h / h);
          const rect = { x: f.rect.x + (f.rect.w - w * s) / 2, y: f.rect.y + (f.rect.h - h * s) / 2, w: w * s, h: h * s };
          mutate("Signing", (docId) => api.addAnnot(docId, pi, { type: "image", rect, data: dataUrlToB64(sig) }, useStore.getState().author));
        }}>
        Sign here
      </div>
    );
  }
  const multiline = f.rect.h > 30;
  const common = {
    className: "form-input",
    "aria-label": f.name,
    style,
    value,
    required: f.required,
    onPointerDown: stop,
    onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => setValue(e.target.value),
    onBlur: () => value !== f.value && set({ kind: "text", value }),
  };
  return multiline ? <textarea {...common} /> : <input type="text" {...common} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />;
}
