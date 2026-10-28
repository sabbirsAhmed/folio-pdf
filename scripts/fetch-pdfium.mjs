// Downloads a prebuilt PDFium library (https://github.com/bblanchon/pdfium-binaries)
// for the build target and stages it in vendor/pdfium-bundle, which Tauri
// bundles into the app's resources.
//
//   node scripts/fetch-pdfium.mjs                # current OS/arch
//   node scripts/fetch-pdfium.mjs --target mac-univ
//   PDFIUM_TAG=chromium/7350 node scripts/fetch-pdfium.mjs
//
// Targets: win-x64, win-arm64, win-x86, mac-univ, mac-arm64, mac-x64, linux-x64, linux-arm64

import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const argTarget = process.argv.includes("--target") ? process.argv[process.argv.indexOf("--target") + 1] : null;

function detect() {
  const arch = os.arch();
  switch (process.platform) {
    case "win32": return arch === "arm64" ? "win-arm64" : arch === "ia32" ? "win-x86" : "win-x64";
    case "darwin": return "mac-univ";
    default: return arch === "arm64" ? "linux-arm64" : "linux-x64";
  }
}

const target = argTarget ?? detect();
const tag = process.env.PDFIUM_TAG;
const url = tag
  ? `https://github.com/bblanchon/pdfium-binaries/releases/download/${encodeURIComponent(tag)}/pdfium-${target}.tgz`
  : `https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-${target}.tgz`;

const work = join(root, "vendor", `pdfium-${target}`);
const bundle = join(root, "vendor", "pdfium-bundle");
const archive = join(root, "vendor", `pdfium-${target}.tgz`);

console.log(`[pdfium] ${target}: ${url}`);
mkdirSync(join(root, "vendor"), { recursive: true });
const res = await fetch(url, { redirect: "follow" });
if (!res.ok) {
  console.error(`[pdfium] download failed: HTTP ${res.status}`);
  process.exit(1);
}
writeFileSync(archive, Buffer.from(await res.arrayBuffer()));

rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
// tar ships with Windows 10+, macOS and Linux.
execFileSync("tar", ["-xzf", archive, "-C", work], { stdio: "inherit" });
rmSync(archive, { force: true });

const libName = target.startsWith("win") ? "pdfium.dll" : target.startsWith("mac") ? "libpdfium.dylib" : "libpdfium.so";
const candidates = [join(work, "bin", libName), join(work, "lib", libName)];
const lib = candidates.find(existsSync);
if (!lib) {
  console.error(`[pdfium] ${libName} not found in archive`);
  process.exit(1);
}

rmSync(bundle, { recursive: true, force: true });
mkdirSync(bundle, { recursive: true });
cpSync(lib, join(bundle, libName));
for (const f of ["LICENSE", "VERSION"]) if (existsSync(join(work, f))) cpSync(join(work, f), join(bundle, `PDFIUM_${f}`));
if (existsSync(join(work, "licenses"))) cpSync(join(work, "licenses"), join(bundle, "licenses"), { recursive: true });
console.log(`[pdfium] staged ${libName} -> vendor/pdfium-bundle (${readdirSync(bundle).length} files)`);
