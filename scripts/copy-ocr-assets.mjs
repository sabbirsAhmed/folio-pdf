// Copies the OCR engine (tesseract.js worker + WASM core) and language data
// into public/ocr so the app runs OCR fully offline.
import { cpSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const nm = (...p) => join(root, "node_modules", ...p);
const out = (...p) => join(root, "public", "ocr", ...p);

const copies = [
  [nm("tesseract.js", "dist", "worker.min.js"), out("worker.min.js")],
  ...["tesseract-core-lstm.wasm.js", "tesseract-core-simd-lstm.wasm.js", "tesseract-core-lstm.js", "tesseract-core-simd-lstm.js"].map((f) => [
    nm("tesseract.js-core", f),
    out("core", f),
  ]),
  [nm("@tesseract.js-data", "eng", "4.0.0_best_int", "eng.traineddata.gz"), out("lang", "eng.traineddata.gz")],
  [nm("@tesseract.js-data", "ben", "4.0.0_best_int", "ben.traineddata.gz"), out("lang", "ben.traineddata.gz")],
];

let n = 0;
for (const [from, to] of copies) {
  if (!existsSync(from)) {
    console.warn(`[ocr] missing ${from}`);
    continue;
  }
  mkdirSync(dirname(to), { recursive: true });
  cpSync(from, to);
  n++;
}
console.log(`[ocr] copied ${n}/${copies.length} OCR assets to public/ocr`);
