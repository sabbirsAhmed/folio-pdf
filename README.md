# Folio PDF

A cross-platform (Windows, macOS) desktop PDF editor with the Acrobat Pro feature set: view, comment, fill and sign, edit text and images, organise pages, build forms, redact, password-protect, reduce size, OCR scanned pages, and export to Word, Excel, images and text.

Built with **Tauri 2** (Rust shell, system webview UI), **PDFium** (Chrome's PDF engine) for rendering and editing, and **lopdf** for structural operations PDFium does not expose.

## Features

| Area | What works |
|---|---|
| Viewing | Tabs, continuous scroll, fit width/page, zoom (Ctrl+wheel), thumbnails, bookmarks, find with match case / whole word, hand tool, text selection and copy, print |
| Comment | Highlight, underline, strikethrough, squiggly, sticky notes, text boxes, freehand ink, line, arrow, rectangle, ellipse, rubber stamps; edit comment text/colour, delete, comments list |
| Fill & sign | Fill text/checkbox/radio/dropdown/list fields; draw, type or upload a signature and place it; "Sign here" on signature fields |
| Edit PDF | Double-click any text run to change it (falls back to a matching standard font when the embedded subset lacks glyphs), move/resize/recolour/delete text, images and graphics, add text and images, replace/extract images, watermark, header/footer, page numbers and Bates numbering, crop |
| Organise | Drag-and-drop reorder, rotate, delete, insert blank or from file, duplicate, extract, split (by count, ranges or size), reverse, combine PDFs and images |
| Forms | Create text, paragraph, checkbox, radio group, dropdown, list and signature fields; required flag; export/import form data (JSON); clear; flatten |
| Protect | True redaction (text removed glyph-by-glyph, image pixels overwritten, overlapping vectors/comments removed), find-and-redact including email/phone patterns, AES-256 password protection with permissions, remove hidden information (metadata, attachments, JavaScript, links) |
| Convert | Word (.docx with paragraphs, bold/italic, sizes, images), Excel (.xlsx, columns detected), PNG/JPEG per page at chosen DPI, plain text, images → PDF |
| OCR | Offline Tesseract (English and Bangla) adds an invisible text layer so scanned pages become searchable and selectable |
| Optimise | Image downsampling/recompression, stream compression, object streams |
| Document | Properties (title, author, subject, keywords), undo/redo (50 steps), unsaved-changes guard, recent files, drag-and-drop open, file association, single instance |

## Project layout

```
crates/engine/       PDF engine (Rust). All PDFium access on one dedicated thread.
  src/annots.rs      comments, stamps, signatures
  src/forms.rs       form fields (read, fill, create, flatten, import/export)
  src/edit.rs        content editing, watermark, header/footer, OCR text layer
  src/redact.rs      true redaction
  src/pages.rs       page operations, split, combine
  src/convert.rs     Word / Excel / images / text export
  src/lo.rs          lopdf-based ops: encryption, field creation, reorder, outline, compress, sanitize
  tests/smoke.rs     end-to-end tests against real PDFium
src-tauri/           Tauri shell: commands, file association, single instance, bundling
src/                 React + TypeScript UI
scripts/             fetch-pdfium.mjs (downloads PDFium), copy-ocr-assets.mjs
.github/workflows/   CI that builds the Windows (.msi/.exe) and macOS (.dmg) installers
```

## Prerequisites

- Node.js 20+ and Rust (stable) — https://rustup.rs
- **Windows:** Visual Studio 2022 Build Tools with "Desktop development with C++"; WebView2 (preinstalled on Windows 10/11)
- **macOS:** Xcode Command Line Tools (`xcode-select --install`)

## Build and run

```bash
npm install                      # also copies OCR engine + language data into public/ocr
npm run fetch-pdfium             # downloads PDFium for this OS into vendor/pdfium-bundle
npm run tauri dev                # run in development
npm run tauri build              # produce installers
```

Installers are written to `target/release/bundle/` (`msi/`, `nsis/` on Windows; `dmg/`, `macos/` on macOS).

Universal macOS build (Apple Silicon + Intel):

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
node scripts/fetch-pdfium.mjs --target mac-univ
npm run tauri build -- --target universal-apple-darwin
```

### Building both platforms from GitHub

Push the repository and run the **Build installers** workflow (or push a tag such as `v0.1.0`). It runs the engine test suite, then builds the Windows and macOS installers and attaches them to a draft release. For signed/notarised macOS builds add the `APPLE_*` secrets listed in the workflow; for Windows code signing see the Tauri signing guide.

## Tests

```bash
npm run fetch-pdfium
FOLIO_PDFIUM_DIR=$PWD/vendor/pdfium-bundle cargo test -p folio-engine
```

The suite exercises create, text, search, every annotation type, undo/redo, all field types, save/reopen, flatten, redaction (synthetic and real TeX-generated PDFs with subset fonts), text editing with font fallback, watermark, header/footer, page operations on rotated pages, metadata, bookmarks, OCR layer, Word/Excel/text/image export, compression, sanitising and AES-256 encryption round trips. Rendered snapshots land in `target/test-out/`.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| Ctrl/Cmd+O, N, S, Shift+S, P, W | Open, new, save, save as, print, close tab |
| Ctrl/Cmd+Z, Y (or Shift+Z) | Undo, redo |
| Ctrl/Cmd+F | Find |
| Ctrl/Cmd + = / − / 0 / 1 / 2 | Zoom in / out / fit page / 100% / fit width |
| PageUp / PageDown | Previous / next page |
| V, H, U, N, P, T, R | Select, hand, highlight, note, draw, text box, rectangle |
| Delete | Delete the selected comment, object or field |
| Esc | Back to the select tool |

## Known limitations

- **Digital (certificate) signatures** are not implemented; signatures are visual (image stamps). PKCS#7 signing needs a certificate store integration.
- **Text editing** changes one text run at a time and does not reflow paragraphs. New characters missing from an embedded font subset switch that run to Helvetica/Times/Courier.
- **Word/Excel export** reconstructs layout from character positions; multi-column layouts and complex tables may need cleanup.
- **Redaction** inside Form XObjects removes the whole XObject that overlaps the area (reported in the result). Text drawn with heavy per-glyph kerning may shift slightly in rebuilt runs.
- **Form field appearances** for new values use Helvetica (WinAnsi); characters outside Latin-1 display as `?` in the appearance but the stored value is correct.
- PDFium is linked dynamically and shipped in the app resources; on macOS sign the bundled `libpdfium.dylib` along with the app when distributing.

## Licences

Folio PDF: MIT. PDFium: BSD-3-Clause (see `vendor/pdfium-bundle/PDFIUM_LICENSE`). Tesseract.js: Apache-2.0. Other dependencies: see their crates/packages.
