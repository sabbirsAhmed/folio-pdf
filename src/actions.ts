import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, errCode, errMsg, type DocInfo } from "./api";
import { useStore, activeTab } from "./store";
import { clearDocCache } from "./render";

const S = () => useStore.getState();

export const PDF_FILTER = [{ name: "PDF documents", extensions: ["pdf"] }];
export const IMAGE_FILTER = [{ name: "Images", extensions: ["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp"] }];

export function baseName(p: string) {
  return p.split(/[\\/]/).pop() ?? p;
}
export function stem(p: string) {
  return baseName(p).replace(/\.[^.]+$/, "");
}
export function dirName(p: string) {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i > 0 ? p.slice(0, i) : p;
}

/** Run an engine operation with a busy indicator and error toast. */
export async function task<T>(label: string, fn: () => Promise<T>): Promise<T | undefined> {
  S().setBusy(label);
  try {
    return await fn();
  } catch (e) {
    S().toast(errMsg(e), "error");
    return undefined;
  } finally {
    S().setBusy(null);
  }
}

/** Apply a mutation to the active document and refresh its view. */
export async function mutate(label: string, fn: (id: number) => Promise<unknown>) {
  const t = activeTab();
  if (!t) return false;
  const id = t.info.id;
  const ok = await task(label, async () => {
    const r = await fn(id);
    const info = r && typeof r === "object" && "page_count" in r ? (r as DocInfo) : await api.info(id);
    S().updateInfo(id, info);
    return true;
  });
  return !!ok;
}

export function rememberRecent(path: string) {
  try {
    const list: string[] = JSON.parse(localStorage.getItem("folio.recent") || "[]");
    const next = [path, ...list.filter((p) => p !== path)].slice(0, 12);
    localStorage.setItem("folio.recent", JSON.stringify(next));
  } catch { /* ignore */ }
}

export function recentFiles(): string[] {
  try {
    return JSON.parse(localStorage.getItem("folio.recent") || "[]");
  } catch {
    return [];
  }
}

export async function openPath(path: string, password?: string): Promise<void> {
  const existing = Object.values(S().tabs).find((t) => t.info.path === path);
  if (existing) {
    S().setActive(existing.info.id);
    return;
  }
  S().setBusy("Opening");
  try {
    const info = await api.open(path, password);
    S().addTab(info);
    rememberRecent(path);
  } catch (e) {
    const code = errCode(e);
    if (code === "password_required" || code === "password_incorrect") {
      S().openDialog("password", { path, retry: code === "password_incorrect" });
    } else {
      S().toast(`${baseName(path)}: ${errMsg(e)}`, "error");
    }
  } finally {
    S().setBusy(null);
  }
}

export async function openFiles() {
  const picked = await openDialog({ multiple: true, filters: PDF_FILTER });
  if (!picked) return;
  for (const p of Array.isArray(picked) ? picked : [picked]) await openPath(p);
}

export async function openAny(paths: string[]) {
  const pdfs = paths.filter((p) => /\.pdf$/i.test(p));
  const images = paths.filter((p) => /\.(png|jpe?g|bmp|gif|tiff?|webp)$/i.test(p));
  for (const p of pdfs) await openPath(p);
  if (images.length) {
    const info = await task("Creating PDF", () => api.combine(images));
    if (info) S().addTab(info);
  }
}

export async function createBlank() {
  const info = await task("Creating", () => api.createBlank(595.28, 841.89, 1));
  if (info) S().addTab(info);
}

export async function saveDoc(id?: number, as = false): Promise<boolean> {
  const t = id != null ? S().tabs[id] : activeTab();
  if (!t) return false;
  let path = t.info.path;
  if (as || !path) {
    const chosen = await saveDialog({ defaultPath: path ?? t.info.name, filters: PDF_FILTER });
    if (!chosen) return false;
    path = /\.pdf$/i.test(chosen) ? chosen : `${chosen}.pdf`;
  }
  const info = await task("Saving", () => api.save(t.info.id, path));
  if (!info) return false;
  S().updateInfo(t.info.id, info);
  rememberRecent(path!);
  S().toast(`Saved ${info.name}`);
  return true;
}

export async function closeDoc(id: number, force = false) {
  const t = S().tabs[id];
  if (!t) return;
  if (t.info.dirty && !force) {
    S().openDialog("unsaved", { id });
    return;
  }
  await api.close(id).catch(() => undefined);
  clearDocCache(id);
  S().closeTab(id);
}

export async function undo() {
  const t = activeTab();
  if (!t?.info.can_undo) return;
  await mutate("Undoing", (id) => api.undo(id));
}

export async function redo() {
  const t = activeTab();
  if (!t?.info.can_redo) return;
  await mutate("Redoing", (id) => api.redo(id));
}

/** Render every page into #print-root and open the system print dialog. */
export async function printDoc() {
  const t = activeTab();
  if (!t) return;
  const root = document.getElementById("print-root")!;
  root.innerHTML = "";
  const urls: string[] = [];
  const ok = await task("Preparing to print", async () => {
    for (const p of t.info.pages) {
      const blob = await api.renderPage(t.info.id, p.index, 2);
      const url = URL.createObjectURL(blob);
      urls.push(url);
      const img = document.createElement("img");
      img.src = url;
      img.className = p.width > p.height ? "landscape" : "";
      root.appendChild(img);
      await img.decode().catch(() => undefined);
    }
    return true;
  });
  if (!ok) return;
  try {
    await api.printWindow();
  } catch {
    window.print();
  }
  setTimeout(() => {
    root.innerHTML = "";
    urls.forEach((u) => URL.revokeObjectURL(u));
  }, 4000);
}

export async function pickSavePath(defaultName: string, filters: { name: string; extensions: string[] }[]) {
  const p = await saveDialog({ defaultPath: defaultName, filters });
  if (!p) return null;
  const ext = filters[0].extensions[0];
  return p.toLowerCase().endsWith(`.${ext}`) ? p : `${p}.${ext}`;
}

export async function pickFolder() {
  const p = await openDialog({ directory: true });
  return typeof p === "string" ? p : null;
}

export function zoomBy(factor: number) {
  const t = activeTab();
  if (!t) return;
  const zoom = Math.min(8, Math.max(0.1, Math.round(t.zoom * factor * 100) / 100));
  S().patchTab(t.info.id, { zoom, fit: null });
}

export function setZoom(zoom: number | "width" | "page") {
  const t = activeTab();
  if (!t) return;
  if (typeof zoom === "number") S().patchTab(t.info.id, { zoom, fit: null });
  else S().patchTab(t.info.id, { fit: zoom });
}

export function goToPage(page: number) {
  const t = activeTab();
  if (!t) return;
  const p = Math.max(0, Math.min(t.info.page_count - 1, page));
  S().patchTab(t.info.id, { page: p });
  S().scrollTo(t.info.id, p);
}
