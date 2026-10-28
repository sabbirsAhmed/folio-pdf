import { useState } from "react";
import { RotateCcw, RotateCw, Trash2, FilePlus2, FileInput, Copy, FileOutput, Scissors, ArrowDownUp } from "lucide-react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useStore } from "../store";
import { api } from "../api";
import { mutate, PDF_FILTER, pickSavePath, stem, openPath } from "../actions";
import { Thumb } from "./Thumb";

export function Organize() {
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null))!;
  const patchTab = useStore((s) => s.patchTab);
  const openDialog = useStore((s) => s.openDialog);
  const [size, setSize] = useState(150);
  const [dropAt, setDropAt] = useState<number | null>(null);
  const [anchor, setAnchor] = useState<number | null>(null);
  const sel = t.selectedPages;
  const setSel = (pages: number[]) => patchTab(t.info.id, { selectedPages: pages });
  const targets = sel.length ? sel : [t.page];
  const n = t.info.page_count;

  const click = (i: number, e: React.MouseEvent) => {
    if (e.shiftKey && anchor != null) {
      const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
      setSel(Array.from({ length: b - a + 1 }, (_, k) => a + k));
    } else if (e.ctrlKey || e.metaKey) {
      setSel(sel.includes(i) ? sel.filter((x) => x !== i) : [...sel, i].sort((a, b) => a - b));
      setAnchor(i);
    } else {
      setSel([i]);
      setAnchor(i);
    }
    patchTab(t.info.id, { page: i });
  };

  const del = () => {
    if (targets.length >= n) {
      useStore.getState().toast("A document needs at least one page.", "error");
      return;
    }
    openDialog("confirm", {
      title: `Delete ${targets.length} page${targets.length > 1 ? "s" : ""}?`,
      body: "You can undo this until you close the document.",
      ok: "Delete",
      danger: true,
      onOk: async () => { if (await mutate("Deleting pages", (id) => api.deletePages(id, targets))) setSel([]); },
    });
  };

  const insertFile = async () => {
    const p = await openFileDialog({ filters: PDF_FILTER });
    if (typeof p !== "string") return;
    const at = (sel.length ? Math.max(...sel) : t.page) + 1;
    await mutate("Inserting pages", (id) => api.insertFromFile(id, at, p, ""));
  };

  const extract = async () => {
    const path = await pickSavePath(`${stem(t.info.name)} - pages.pdf`, PDF_FILTER);
    if (!path) return;
    const ok = await mutate("Extracting", (id) => api.extractPages(id, targets, path, false));
    if (ok) useStore.getState().toast(`Saved ${targets.length} page${targets.length > 1 ? "s" : ""}`, "info", { label: "Open", run: () => openPath(path) });
  };

  const onDrop = async (to: number) => {
    setDropAt(null);
    const moving = sel.length ? sel : [];
    if (!moving.length) return;
    const before = moving.filter((p) => p < to).length;
    const dest = to - before;
    await mutate("Moving pages", (id) => api.movePages(id, moving, dest));
    setSel(moving.map((_, k) => dest + k));
  };

  return (
    <div className="organize">
      <div className="org-bar">
        <button className="ibtn" title="Rotate left" onClick={() => mutate("Rotating", (id) => api.rotatePages(id, targets, -90))}><RotateCcw size={17} /> Left</button>
        <button className="ibtn" title="Rotate right" onClick={() => mutate("Rotating", (id) => api.rotatePages(id, targets, 90))}><RotateCw size={17} /> Right</button>
        <button className="ibtn danger" title="Delete pages" onClick={del}><Trash2 size={17} /> Delete</button>
        <span className="sep" />
        <button className="ibtn" title="Insert blank page after selection" onClick={() => {
          const ref = t.info.pages[targets[targets.length - 1]] ?? t.info.pages[0];
          mutate("Inserting page", (id) => api.insertBlank(id, targets[targets.length - 1] + 1, ref.width, ref.height));
        }}><FilePlus2 size={17} /> Blank page</button>
        <button className="ibtn" title="Insert pages from a file" onClick={insertFile}><FileInput size={17} /> From file</button>
        <button className="ibtn" title="Duplicate pages" onClick={() => mutate("Duplicating", (id) => api.duplicatePages(id, targets))}><Copy size={17} /> Duplicate</button>
        <span className="sep" />
        <button className="ibtn" title="Save selected pages as a new PDF" onClick={extract}><FileOutput size={17} /> Extract</button>
        <button className="ibtn" title="Split into several files" onClick={() => openDialog("split")}><Scissors size={17} /> Split</button>
        <button className="ibtn" title="Reverse page order" onClick={() => mutate("Reversing", (id) => api.reversePages(id))}><ArrowDownUp size={17} /> Reverse</button>
        <span style={{ flex: 1 }} />
        <span className="muted">{sel.length ? `${sel.length} selected` : "Click to select, drag to reorder"}</span>
        <input type="range" min={90} max={300} value={size} aria-label="Thumbnail size" onChange={(e) => setSize(+e.target.value)} />
      </div>
      <div className="org-grid" onDragOver={(e) => e.preventDefault()} onDrop={() => dropAt != null && onDrop(dropAt)}>
        {t.info.pages.map((p) => (
          <div
            key={p.index}
            className={`org-item ${sel.includes(p.index) ? "sel" : ""} ${dropAt === p.index ? "drop-before" : ""}`}
            draggable
            onDragStart={(e) => {
              if (!sel.includes(p.index)) setSel([p.index]);
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData("text/plain", String(p.index));
            }}
            onDragOver={(e) => {
              e.preventDefault();
              const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
              setDropAt(e.clientX < r.left + r.width / 2 ? p.index : p.index + 1);
            }}
            onDragEnd={() => setDropAt(null)}
            onClick={(e) => click(p.index, e)}
            onDoubleClick={() => { patchTab(t.info.id, { mode: "view" }); useStore.getState().scrollTo(t.info.id, p.index); }}
          >
            <Thumb id={t.info.id} version={t.version} page={p} width={p.width >= p.height ? size * 1.3 : size} />
            <div className="actions">
              <button className="ibtn small" title="Rotate left" onClick={(e) => { e.stopPropagation(); mutate("Rotating", (id) => api.rotatePages(id, [p.index], -90)); }}><RotateCcw size={14} /></button>
              <button className="ibtn small" title="Rotate right" onClick={(e) => { e.stopPropagation(); mutate("Rotating", (id) => api.rotatePages(id, [p.index], 90)); }}><RotateCw size={14} /></button>
              <button className="ibtn small danger" title="Delete page" onClick={(e) => { e.stopPropagation(); if (n > 1) mutate("Deleting page", (id) => api.deletePages(id, [p.index])); }}><Trash2 size={14} /></button>
            </div>
            <span className="num">{p.label || p.index + 1}</span>
          </div>
        ))}
        {dropAt === n && <div style={{ width: 3, alignSelf: "stretch", background: "var(--accent)" }} />}
        <div style={{ width: 40, height: size }} onDragOver={(e) => { e.preventDefault(); setDropAt(n); }} />
      </div>
    </div>
  );
}
