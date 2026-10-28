import { create } from "zustand";
import type { DocInfo, Rect, SearchHit } from "./api";

export type Tool =
  | "select" | "hand"
  | "highlight" | "underline" | "strikeout" | "squiggly"
  | "note" | "textbox" | "ink" | "rect" | "ellipse" | "line" | "arrow" | "stamp"
  | "sign" | "addtext" | "addimage" | "edit"
  | "field" | "redact" | "crop";

export type SidePanel = "thumbs" | "bookmarks" | "comments" | "search" | "fields" | null;

export interface Redaction { page: number; rect: Rect }

export interface DocTab {
  info: DocInfo;
  /** Bumped after every change so pages, thumbnails and overlays refetch. */
  version: number;
  zoom: number;
  fit: "width" | "page" | null;
  page: number;
  mode: "view" | "organize";
  redactions: Redaction[];
  hits: SearchHit[];
  hitIndex: number;
  selectedPages: number[];
}

export interface ToolOptions {
  color: string;
  width: number;
  fontSize: number;
  stampLabel: string;
  fieldKind: string;
  signature: string | null; // PNG data URL
  initials: string | null;
  cropAll: boolean;
  markupColor: string;
}

interface Toast { id: number; text: string; kind: "info" | "error"; action?: { label: string; run: () => void } }

export interface DialogState { kind: string; props?: Record<string, unknown> }

interface Store {
  tabs: Record<number, DocTab>;
  order: number[];
  active: number | null;
  tool: Tool;
  opts: ToolOptions;
  panel: SidePanel;
  toolsPane: boolean;
  busy: string | null;
  toasts: Toast[];
  dialog: DialogState | null;
  author: string;
  scrollRequest: { id: number; page: number; y?: number; nonce: number } | null;
  selAnnot: { id: number; page: number; index: number } | null;
  setSelAnnot: (a: { id: number; page: number; index: number } | null) => void;

  addTab: (info: DocInfo) => void;
  closeTab: (id: number) => void;
  setActive: (id: number | null) => void;
  updateInfo: (id: number, info: DocInfo) => void;
  bump: (id: number) => void;
  patchTab: (id: number, patch: Partial<DocTab>) => void;
  setTool: (t: Tool) => void;
  setOpts: (o: Partial<ToolOptions>) => void;
  setPanel: (p: SidePanel) => void;
  toggleToolsPane: () => void;
  setBusy: (s: string | null) => void;
  toast: (text: string, kind?: "info" | "error", action?: Toast["action"]) => void;
  dismiss: (id: number) => void;
  openDialog: (kind: string, props?: Record<string, unknown>) => void;
  closeDialog: () => void;
  setAuthor: (a: string) => void;
  scrollTo: (id: number, page: number, y?: number) => void;
}

const loadOpts = (): ToolOptions => {
  const base: ToolOptions = { color: "#d6336c", width: 2, fontSize: 12, stampLabel: "APPROVED", fieldKind: "text", signature: null, initials: null, cropAll: false, markupColor: "#ffd43b" };
  try {
    const raw = localStorage.getItem("folio.opts");
    if (raw) return { ...base, ...JSON.parse(raw) };
  } catch { /* storage unavailable */ }
  return base;
};

let toastSeq = 1;

export const useStore = create<Store>((set, get) => ({
  tabs: {},
  order: [],
  active: null,
  tool: "select",
  opts: loadOpts(),
  panel: "thumbs",
  toolsPane: true,
  busy: null,
  toasts: [],
  dialog: null,
  author: "User",
  scrollRequest: null,
  selAnnot: null,
  setSelAnnot: (selAnnot) => set({ selAnnot }),

  addTab: (info) =>
    set((s) => ({
      tabs: { ...s.tabs, [info.id]: { info, version: 1, zoom: 1, fit: "width", page: 0, mode: "view", redactions: [], hits: [], hitIndex: 0, selectedPages: [] } },
      order: [...s.order, info.id],
      active: info.id,
    })),
  closeTab: (id) =>
    set((s) => {
      const tabs = { ...s.tabs };
      delete tabs[id];
      const order = s.order.filter((x) => x !== id);
      const idx = s.order.indexOf(id);
      const active = s.active === id ? order[Math.min(idx, order.length - 1)] ?? null : s.active;
      return { tabs, order, active };
    }),
  setActive: (id) => set({ active: id }),
  updateInfo: (id, info) =>
    set((s) => {
      const t = s.tabs[id];
      if (!t) return {};
      const page = Math.min(t.page, info.page_count - 1);
      return { tabs: { ...s.tabs, [id]: { ...t, info, version: t.version + 1, page, selectedPages: t.selectedPages.filter((p) => p < info.page_count) } } };
    }),
  bump: (id) =>
    set((s) => {
      const t = s.tabs[id];
      return t ? { tabs: { ...s.tabs, [id]: { ...t, version: t.version + 1 } } } : {};
    }),
  patchTab: (id, patch) =>
    set((s) => {
      const t = s.tabs[id];
      return t ? { tabs: { ...s.tabs, [id]: { ...t, ...patch } } } : {};
    }),
  setTool: (tool) => set({ tool }),
  setOpts: (o) => {
    const opts = { ...get().opts, ...o };
    try { localStorage.setItem("folio.opts", JSON.stringify(opts)); } catch { /* ignore */ }
    set({ opts });
  },
  setPanel: (panel) => set((s) => ({ panel: s.panel === panel ? null : panel })),
  toggleToolsPane: () => set((s) => ({ toolsPane: !s.toolsPane })),
  setBusy: (busy) => set({ busy }),
  toast: (text, kind = "info", action) => {
    const id = toastSeq++;
    set((s) => ({ toasts: [...s.toasts, { id, text, kind, action }] }));
    setTimeout(() => get().dismiss(id), kind === "error" ? 7000 : action ? 8000 : 3500);
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  openDialog: (kind, props) => set({ dialog: { kind, props } }),
  closeDialog: () => set({ dialog: null }),
  setAuthor: (author) => set({ author }),
  scrollTo: (id, page, y) => set({ scrollRequest: { id, page, y, nonce: Math.random() } }),
}));

export const activeTab = () => {
  const s = useStore.getState();
  return s.active != null ? s.tabs[s.active] ?? null : null;
};
