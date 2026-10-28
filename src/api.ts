import { invoke } from "@tauri-apps/api/core";

export interface Rect { x: number; y: number; w: number; h: number }
export interface PageInfo { index: number; width: number; height: number; rotation: number; label: string | null }
export interface DocInfo {
  id: number; name: string; path: string | null; page_count: number; pages: PageInfo[];
  title: string; author: string; subject: string; keywords: string; creator: string; producer: string;
  created: string; modified: string; version: string; has_forms: boolean; protected: boolean;
  dirty: boolean; can_undo: boolean; can_redo: boolean;
}
export interface OutlineItem { title: string; page: number | null; children: OutlineItem[] }
export interface TextChar { c: string; r: Rect; g: boolean; size: number }
export interface SearchHit { page: number; rects: Rect[]; context: string }
export interface AnnotInfo { index: number; kind: string; rect: Rect; color: string; contents: string; author: string; modified: string }
export interface FieldInfo {
  page: number; annot_index: number; name: string; kind: string; rect: Rect; value: string;
  checked: boolean; options: string[]; read_only: boolean; required: boolean;
}
export interface ObjInfo { index: number; kind: string; rect: Rect; text: string | null; font_size: number; font_name: string; color: string }
export interface Security {
  user_password: string; owner_password: string; allow_print: boolean; allow_copy: boolean;
  allow_modify: boolean; allow_annotate: boolean; allow_forms: boolean;
}
export interface EngineError { code: string; message: string }

export type FontFamily = "helvetica" | "times" | "courier";
export type NewAnnot =
  | { type: "markup"; style: "highlight" | "underline" | "strikeout" | "squiggly"; rects: Rect[]; color: string; note?: string }
  | { type: "ink"; strokes: [number, number][][]; color: string; width: number }
  | { type: "shape"; shape: "rect" | "ellipse"; rect: Rect; color: string; width: number; fill?: string | null }
  | { type: "line"; from: [number, number]; to: [number, number]; color: string; width: number; arrow: boolean }
  | { type: "note"; x: number; y: number; text: string; color: string }
  | { type: "text_box"; rect: Rect; text: string; size: number; color: string; family?: FontFamily; bold?: boolean; border?: string | null }
  | { type: "stamp"; rect: Rect; label: string; color: string }
  | { type: "image"; rect: Rect; data: string };

export type FieldValue = { kind: "text"; value: string } | { kind: "check"; checked: boolean } | { kind: "choice"; value: string };

export function errMsg(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as EngineError).message);
  return String(e);
}
export function errCode(e: unknown): string {
  if (e && typeof e === "object" && "code" in e) return String((e as EngineError).code);
  return "";
}

const call = <T,>(cmd: string, args: Record<string, unknown> = {}) => invoke<T>(cmd, args);

export const api = {
  open: (path: string, password?: string | null) => call<DocInfo>("open_file", { path, password: password ?? null }),
  createBlank: (width: number, height: number, pages: number) => call<DocInfo>("create_blank", { width, height, pages }),
  close: (id: number) => call<void>("close_doc", { id }),
  info: (id: number) => call<DocInfo>("doc_info", { id }),
  outline: (id: number) => call<OutlineItem[]>("outline", { id }),
  setOutline: (id: number, items: OutlineItem[]) => call<DocInfo>("set_outline", { id, items }),
  save: (id: number, path?: string | null) => call<DocInfo>("save", { id, path: path ?? null }),
  saveCopy: (id: number, path: string) => call<void>("save_copy", { id, path }),
  undo: (id: number) => call<DocInfo>("undo", { id }),
  redo: (id: number) => call<DocInfo>("redo", { id }),
  setMetadata: (id: number, meta: Partial<Record<"title" | "author" | "subject" | "keywords" | "creator", string>>) => call<DocInfo>("set_metadata", { id, meta }),
  setSecurity: (id: number, security: Security | null) => call<DocInfo>("set_security", { id, security }),
  sanitize: (id: number, opts: { metadata: boolean; attachments: boolean; javascript: boolean; links: boolean }) => call<DocInfo>("sanitize", { id, opts }),
  compress: (id: number, level: "low" | "medium" | "high") => call<{ before: number; after: number; images_recompressed: number }>("compress", { id, level }),

  renderPage: async (id: number, page: number, scale: number) => {
    const buf = await call<ArrayBuffer>("render_page", { id, page, scale, annotations: true, forms: true });
    return new Blob([buf], { type: "image/bmp" });
  },
  renderPng: async (id: number, page: number, scale: number) => {
    const buf = await call<ArrayBuffer>("render_png", { id, page, scale });
    return new Blob([buf], { type: "image/png" });
  },
  pageChars: (id: number, page: number) => call<TextChar[]>("page_chars", { id, page }),
  pageHasText: (id: number, page: number) => call<boolean>("page_has_text", { id, page }),
  search: (id: number, query: string, matchCase: boolean, wholeWord: boolean) => call<SearchHit[]>("search", { id, query, matchCase, wholeWord }),

  listAnnots: (id: number, page: number) => call<AnnotInfo[]>("list_annots", { id, page }),
  addAnnot: (id: number, page: number, spec: NewAnnot, author: string) => call<void>("add_annot", { id, page, spec, author }),
  deleteAnnot: (id: number, page: number, index: number) => call<void>("delete_annot", { id, page, index }),
  updateAnnot: (id: number, page: number, index: number, u: { contents?: string; color?: string; rect?: Rect }) =>
    call<void>("update_annot", { id, page, index, contents: u.contents ?? null, color: u.color ?? null, rect: u.rect ?? null }),
  deleteAllAnnots: (id: number, kinds: string[]) => call<number>("delete_all_annots", { id, kinds }),

  listFields: (id: number, page: number) => call<FieldInfo[]>("list_fields", { id, page }),
  setField: (id: number, page: number, index: number, value: FieldValue) => call<void>("set_field", { id, page, index, value }),
  addField: (id: number, spec: { page: number; kind: string; name: string; rect: Rect; options?: string[]; required?: boolean; font_size?: number; export_value?: string }) =>
    call<void>("add_field", { id, spec: { options: [], required: false, font_size: 0, export_value: "", ...spec } }),
  deleteField: (id: number, page: number, index: number) => call<void>("delete_field", { id, page, index }),
  flatten: (id: number) => call<DocInfo>("flatten", { id }),
  resetForm: (id: number) => call<void>("reset_form", { id }),
  exportFormData: (id: number, path: string) => call<void>("export_form_data", { id, path }),
  importFormData: (id: number, path: string) => call<number>("import_form_data", { id, path }),

  rotatePages: (id: number, pages: number[], delta: number) => call<DocInfo>("rotate_pages", { id, pages, delta }),
  deletePages: (id: number, pages: number[]) => call<DocInfo>("delete_pages", { id, pages }),
  insertBlank: (id: number, at: number, width: number, height: number) => call<DocInfo>("insert_blank", { id, at, width, height }),
  insertFromFile: (id: number, at: number, path: string, pages: string) => call<DocInfo>("insert_from_file", { id, at, path, pages, password: null }),
  duplicatePages: (id: number, pages: number[]) => call<DocInfo>("duplicate_pages", { id, pages }),
  movePages: (id: number, pages: number[], to: number) => call<DocInfo>("move_pages", { id, pages, to }),
  reversePages: (id: number) => call<DocInfo>("reverse_pages", { id }),
  extractPages: (id: number, pages: number[], path: string, deleteAfter: boolean) => call<DocInfo>("extract_pages", { id, pages, path, deleteAfter }),
  split: (id: number, o: { every?: number; ranges?: string; maxMb?: number }, outDir: string, stem: string) =>
    call<string[]>("split", { id, every: o.every ?? null, ranges: o.ranges ?? null, maxMb: o.maxMb ?? null, outDir, stem }),
  combine: (files: string[]) => call<DocInfo>("combine", { files }),
  cropPages: (id: number, pages: number[], rect: Rect) => call<DocInfo>("crop_pages", { id, pages, rect }),

  listObjects: (id: number, page: number, includePaths = false) => call<ObjInfo[]>("list_objects", { id, page, includePaths }),
  setObjectText: (id: number, page: number, obj: number, text: string) => call<void>("set_object_text", { id, page, obj, text }),
  moveObject: (id: number, page: number, obj: number, dx: number, dy: number) => call<void>("move_object", { id, page, obj, dx, dy }),
  resizeObject: (id: number, page: number, obj: number, rect: Rect) => call<void>("resize_object", { id, page, obj, rect }),
  deleteObjects: (id: number, page: number, objs: number[]) => call<void>("delete_objects", { id, page, objs }),
  setObjectColor: (id: number, page: number, obj: number, color: string) => call<void>("set_object_color", { id, page, obj, color }),
  addText: (id: number, page: number, text: { x: number; y: number; text: string; size: number; color: string; family?: FontFamily; bold?: boolean; italic?: boolean }) =>
    call<void>("add_text", { id, page, text }),
  addImage: (id: number, page: number, rect: Rect, src: { path?: string; data?: string }) => call<void>("add_image", { id, page, rect, path: src.path ?? null, data: src.data ?? null }),
  replaceImage: (id: number, page: number, obj: number, path: string) => call<void>("replace_image", { id, page, obj, path }),
  extractImage: (id: number, page: number, obj: number, path: string) => call<void>("extract_image", { id, page, obj, path }),
  watermark: (id: number, spec: { text: string; size: number; color: string; angle: number; pages: string; behind: boolean }) => call<void>("watermark", { id, spec }),
  headerFooter: (id: number, spec: Record<string, unknown>, name: string) => call<void>("header_footer", { id, spec, name }),
  addOcrLayer: (id: number, page: number, words: { text: string; x: number; y: number; w: number; h: number }[]) => call<number>("add_ocr_layer", { id, page, words }),
  applyRedactions: (id: number, areas: { page: number; rect: Rect }[], fill: string) => call<Record<string, number>>("apply_redactions", { id, areas, fill }),

  exportDocx: (id: number, path: string, images: boolean) => call<void>("export_docx", { id, path, images }),
  exportXlsx: (id: number, path: string) => call<void>("export_xlsx", { id, path }),
  exportText: (id: number, path: string) => call<void>("export_text", { id, path }),
  exportImages: (id: number, pages: number[], dpi: number, format: string, outDir: string, stem: string) => call<string[]>("export_images", { id, pages, dpi, format, outDir, stem }),

  takePendingFiles: () => call<string[]>("take_pending_files"),
  printWindow: () => call<void>("print_window"),
  reveal: (path: string) => call<void>("reveal_in_folder", { path }),
  openExternally: (path: string) => call<void>("open_externally", { path }),
  userName: () => call<string>("user_name"),
  readImageDataUrl: (path: string) => call<string>("read_image_data_url", { path }),
};
