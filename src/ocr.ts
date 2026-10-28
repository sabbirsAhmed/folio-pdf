import { createWorker, type Worker } from "tesseract.js";
import { api } from "./api";

let worker: Worker | null = null;
let workerLangs = "";

async function getWorker(langs: string, onProgress: (p: number) => void) {
  if (worker && workerLangs === langs) return worker;
  if (worker) await worker.terminate();
  const base = new URL("./ocr/", window.location.href).href;
  worker = await createWorker(langs, 1, {
    workerPath: `${base}worker.min.js`,
    corePath: `${base}core`,
    langPath: `${base}lang`,
    gzip: true,
    workerBlobURL: false,
    logger: (m: { status: string; progress: number }) => {
      if (m.status === "recognizing text") onProgress(m.progress);
    },
  });
  workerLangs = langs;
  return worker;
}

/**
 * OCR the given pages and add an invisible text layer to each, making
 * scanned pages searchable and selectable.
 */
export async function ocrPages(
  id: number,
  pages: number[],
  langs: string,
  onStatus: (page: number, done: number, progress: number) => void,
): Promise<number> {
  let pageProgress = 0;
  let current = 0;
  let done = 0;
  const w = await getWorker(langs, (p) => {
    pageProgress = p;
    onStatus(current, done, pageProgress);
  });
  let words = 0;
  const SCALE = 300 / 72;
  for (const p of pages) {
    current = p;
    onStatus(p, done, 0);
    const png = await api.renderPng(id, p, SCALE);
    const url = URL.createObjectURL(png);
    try {
      const { data } = await w.recognize(url);
      const list = (data.words ?? [])
        .filter((wd) => wd.text.trim() && wd.confidence > 35)
        .map((wd) => ({
          text: wd.text,
          x: wd.bbox.x0 / SCALE,
          y: wd.bbox.y0 / SCALE,
          w: (wd.bbox.x1 - wd.bbox.x0) / SCALE,
          h: (wd.bbox.y1 - wd.bbox.y0) / SCALE,
        }));
      if (list.length) words += await api.addOcrLayer(id, p, list);
    } finally {
      URL.revokeObjectURL(url);
    }
    done++;
    onStatus(p, done, 1);
  }
  return words;
}
