import { useEffect, useMemo, useState } from "react";
import { Bookmark, ChevronDown, ChevronRight, FormInput, Images, MessageSquare, Plus, Search, Trash2, Pencil } from "lucide-react";
import { useStore, type SidePanel as Panel } from "../store";
import { api, type AnnotInfo, type FieldInfo, type OutlineItem } from "../api";
import { goToPage, mutate, task } from "../actions";
import { Thumb } from "./Thumb";

const KIND_LABEL: Record<string, string> = {
  highlight: "Highlight", underline: "Underline", strikeout: "Strikethrough", squiggly: "Squiggly", note: "Note",
  free_text: "Text box", ink: "Drawing", square: "Rectangle", circle: "Ellipse", stamp: "Stamp", line: "Line",
  polygon: "Polygon", polyline: "Polyline", attachment: "Attachment", redact: "Redaction", other: "Comment",
};

export function SideRail() {
  const panel = useStore((s) => s.panel);
  const setPanel = useStore((s) => s.setPanel);
  const btn = (p: Panel, icon: JSX.Element, label: string) => (
    <button className={`ibtn ${panel === p ? "on" : ""}`} title={label} aria-pressed={panel === p} onClick={() => setPanel(p)}>{icon}</button>
  );
  return (
    <nav className="rail" aria-label="Panels">
      {btn("thumbs", <Images size={18} />, "Page thumbnails")}
      {btn("bookmarks", <Bookmark size={18} />, "Bookmarks")}
      {btn("comments", <MessageSquare size={18} />, "Comments")}
      {btn("search", <Search size={18} />, "Find")}
      {btn("fields", <FormInput size={18} />, "Form fields")}
    </nav>
  );
}

export function SidePanel() {
  const panel = useStore((s) => s.panel);
  const titles: Record<string, string> = { thumbs: "Pages", bookmarks: "Bookmarks", comments: "Comments", search: "Find", fields: "Form fields" };
  return (
    <aside className="side">
      <div className="side-head"><span className="grow">{titles[panel ?? ""]}</span></div>
      <div className="side-body">
        {panel === "thumbs" && <Thumbs />}
        {panel === "bookmarks" && <Bookmarks />}
        {panel === "comments" && <Comments />}
        {panel === "search" && <SearchPanel />}
        {panel === "fields" && <Fields />}
      </div>
    </aside>
  );
}

function Thumbs() {
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null))!;
  return (
    <div className="thumbs">
      {t.info.pages.map((p) => (
        <div key={p.index} className={`thumb ${t.page === p.index ? "current" : ""}`} onClick={() => goToPage(p.index)}>
          <Thumb id={t.info.id} version={t.version} page={p} width={p.width >= p.height ? 150 : 110} />
          <span>{p.label || p.index + 1}</span>
        </div>
      ))}
    </div>
  );
}

function Bookmarks() {
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null))!;
  const openDialog = useStore((s) => s.openDialog);
  const [items, setItems] = useState<OutlineItem[] | null>(null);
  useEffect(() => {
    api.outline(t.info.id).then(setItems).catch(() => setItems([]));
  }, [t.info.id, t.version]);

  const save = (next: OutlineItem[]) => mutate("Updating bookmarks", (id) => api.setOutline(id, next));
  const addHere = () =>
    openDialog("prompt", {
      title: "Add bookmark",
      label: "Bookmark name",
      value: `Page ${t.page + 1}`,
      onOk: (title: string) => save([...(items ?? []), { title, page: t.page, children: [] }]),
    });
  const edit = (path: number[], f: (list: OutlineItem[], i: number) => void) => {
    const next = structuredClone(items ?? []);
    let list = next;
    for (const i of path.slice(0, -1)) list = list[i].children;
    f(list, path[path.length - 1]);
    save(next);
  };

  return (
    <div>
      <button className="btn" style={{ width: "100%", marginBottom: 8 }} onClick={addHere}><Plus size={15} /> Bookmark this page</button>
      {items === null ? <div className="empty">Loading…</div> : items.length === 0 ? (
        <div className="empty">This document has no bookmarks.</div>
      ) : (
        <OutlineTree
          items={items}
          path={[]}
          onRename={(path, title) => edit(path, (l, i) => { l[i].title = title; })}
          onDelete={(path) => edit(path, (l, i) => { l.splice(i, 1); })}
        />
      )}
    </div>
  );
}

function OutlineTree({ items, path, onRename, onDelete }: { items: OutlineItem[]; path: number[]; onRename: (p: number[], t: string) => void; onDelete: (p: number[]) => void }) {
  const [open, setOpen] = useState<Record<number, boolean>>({});
  const openDialog = useStore((s) => s.openDialog);
  return (
    <div>
      {items.map((it, i) => (
        <div key={i}>
          <div className="outline-item" onClick={() => it.page != null && goToPage(it.page)}>
            {it.children.length ? (
              <button className="ibtn small" onClick={(e) => { e.stopPropagation(); setOpen({ ...open, [i]: !open[i] }); }} aria-label="Expand">
                {open[i] ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              </button>
            ) : <span style={{ width: 24, flex: "none" }} />}
            <span style={{ flex: 1, paddingTop: 3 }}>{it.title}</span>
            <button className="ibtn small" title="Rename" onClick={(e) => { e.stopPropagation(); openDialog("prompt", { title: "Rename bookmark", label: "Bookmark name", value: it.title, onOk: (v: string) => onRename([...path, i], v) }); }}><Pencil size={13} /></button>
            <button className="ibtn small" title="Delete bookmark" onClick={(e) => { e.stopPropagation(); onDelete([...path, i]); }}><Trash2 size={13} /></button>
          </div>
          {open[i] && it.children.length > 0 && (
            <div className="outline-children">
              <OutlineTree items={it.children} path={[...path, i]} onRename={onRename} onDelete={onDelete} />
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function Comments() {
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null))!;
  const sel = useStore((s) => s.selAnnot);
  const setSel = useStore((s) => s.setSelAnnot);
  const [list, setList] = useState<(AnnotInfo & { page: number })[] | null>(null);
  useEffect(() => {
    let live = true;
    (async () => {
      const out: (AnnotInfo & { page: number })[] = [];
      for (const p of t.info.pages) {
        const a = await api.listAnnots(t.info.id, p.index).catch(() => []);
        out.push(...a.filter((x) => x.kind !== "link").map((x) => ({ ...x, page: p.index })));
        if (!live) return;
      }
      if (live) setList(out);
    })();
    return () => { live = false; };
  }, [t.info.id, t.version]);

  if (!list) return <div className="empty">Loading…</div>;
  if (!list.length) return <div className="empty">No comments yet. Pick Highlight, Sticky note or Draw to add one.</div>;
  return (
    <div>
      <div className="muted" style={{ display: "flex", alignItems: "center", marginBottom: 6 }}>
        <span style={{ flex: 1 }}>{list.length} comment{list.length > 1 ? "s" : ""}</span>
        <button className="ibtn small danger" title="Delete all comments" onClick={() => useStore.getState().openDialog("confirm", {
          title: "Delete all comments?", body: "Highlights, notes, drawings and stamps are removed from every page.", ok: "Delete all", danger: true,
          onOk: () => mutate("Deleting comments", (id) => api.deleteAllAnnots(id, [])),
        })}><Trash2 size={14} /></button>
      </div>
      {list.map((a) => {
        const on = sel && sel.id === t.info.id && sel.page === a.page && sel.index === a.index;
        return (
          <div key={`${a.page}:${a.index}`} className={`list-item ${on ? "on" : ""}`} onClick={() => { goToPage(a.page); setSel({ id: t.info.id, page: a.page, index: a.index }); }}>
            <span className="kind-dot" style={{ background: a.color || "#adb5bd" }} />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div>{KIND_LABEL[a.kind] ?? a.kind}</div>
              {a.contents && <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{a.contents}</div>}
              <div className="meta">Page {a.page + 1}{a.author ? `, ${a.author}` : ""}</div>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function SearchPanel() {
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null))!;
  const patchTab = useStore((s) => s.patchTab);
  const [q, setQ] = useState("");
  const [mc, setMc] = useState(false);
  const [ww, setWw] = useState(false);
  const [done, setDone] = useState(false);
  const hits = t.hits;
  const run = async () => {
    if (!q.trim()) return;
    const r = await task("Searching", () => api.search(t.info.id, q, mc, ww));
    if (r) {
      patchTab(t.info.id, { hits: r, hitIndex: 0 });
      setDone(true);
      if (r.length) goToPage(r[0].page);
    }
  };
  const go = (i: number) => {
    if (!hits.length) return;
    const n = (i + hits.length) % hits.length;
    patchTab(t.info.id, { hitIndex: n });
    const h = hits[n];
    useStore.getState().scrollTo(t.info.id, h.page, h.rects[0]?.y);
  };
  const ctx = useMemo(() => (h: string) => {
    const i = h.toLowerCase().indexOf(q.toLowerCase());
    if (i < 0 || !q) return <>{h}</>;
    return <>{h.slice(0, i)}<b>{h.slice(i, i + q.length)}</b>{h.slice(i + q.length)}</>;
  }, [q]);
  return (
    <div>
      <div className="search-form">
        <input id="search-input" className="field-input" type="text" placeholder="Find in document" value={q} onChange={(e) => { setQ(e.target.value); setDone(false); }}
          onKeyDown={(e) => { if (e.key === "Enter") (done ? go(t.hitIndex + (e.shiftKey ? -1 : 1)) : run()); if (e.key === "Escape") patchTab(t.info.id, { hits: [] }); }} />
        <label><input type="checkbox" checked={mc} onChange={(e) => setMc(e.target.checked)} /> Match case</label>
        <label><input type="checkbox" checked={ww} onChange={(e) => setWw(e.target.checked)} /> Whole words</label>
        <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
          <button className="btn primary" onClick={run}>Find</button>
          {hits.length > 0 && (
            <>
              <button className="ibtn" onClick={() => go(t.hitIndex - 1)} aria-label="Previous match"><ChevronDown size={15} style={{ transform: "rotate(180deg)" }} /></button>
              <button className="ibtn" onClick={() => go(t.hitIndex + 1)} aria-label="Next match"><ChevronDown size={15} /></button>
              <span className="muted">{t.hitIndex + 1} of {hits.length}</span>
            </>
          )}
        </div>
      </div>
      {done && !hits.length && <div className="empty">No matches. Scanned pages need OCR before they can be searched.</div>}
      {hits.map((h, i) => (
        <div key={i} className={`hit ${i === t.hitIndex ? "on" : ""}`} onClick={() => go(i)}>
          <div className="faint">Page {h.page + 1}</div>
          <div>{ctx(h.context)}</div>
        </div>
      ))}
    </div>
  );
}

function Fields() {
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null))!;
  const [list, setList] = useState<FieldInfo[] | null>(null);
  useEffect(() => {
    let live = true;
    (async () => {
      const out: FieldInfo[] = [];
      for (const p of t.info.pages) out.push(...(await api.listFields(t.info.id, p.index).catch(() => [])));
      if (live) setList(out);
    })();
    return () => { live = false; };
  }, [t.info.id, t.version]);
  if (!list) return <div className="empty">Loading…</div>;
  if (!list.length) return <div className="empty">No form fields. Use Prepare form to add some.</div>;
  return (
    <div>
      {list.map((f) => (
        <div key={`${f.page}:${f.annot_index}`} className="list-item" onClick={() => useStore.getState().scrollTo(t.info.id, f.page, f.rect.y)}>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div>{f.name}{f.required ? " *" : ""}</div>
            <div className="meta">{f.kind}, page {f.page + 1}{f.kind === "checkbox" ? (f.checked ? ", checked" : "") : f.value ? `: ${f.value}` : ""}</div>
          </div>
        </div>
      ))}
    </div>
  );
}
