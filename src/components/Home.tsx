import { useState } from "react";
import { FolderOpen, FilePlus2, Combine, Images, FileText, X } from "lucide-react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useStore } from "../store";
import { openFiles, createBlank, recentFiles, openPath, baseName, dirName, openAny, IMAGE_FILTER } from "../actions";

export function Home() {
  const openDialog = useStore((s) => s.openDialog);
  const [recent, setRecent] = useState(recentFiles());
  const forget = (p: string) => {
    const next = recent.filter((x) => x !== p);
    setRecent(next);
    try { localStorage.setItem("folio.recent", JSON.stringify(next)); } catch { /* ignore */ }
  };
  return (
    <main className="home">
      <div className="home-inner">
        <div>
          <div className="brand">
            <div className="brand-mark" aria-hidden><i /><i /><i /></div>
            <h1>Folio PDF</h1>
          </div>
          <div className="home-actions">
            <button className="home-action" onClick={openFiles}>
              <FolderOpen size={22} /><span>Open a PDF<small>Ctrl+O</small></span>
            </button>
            <button className="home-action" onClick={createBlank}>
              <FilePlus2 size={22} /><span>New blank PDF<small>Ctrl+N</small></span>
            </button>
            <button className="home-action" onClick={() => openDialog("combine")}>
              <Combine size={22} /><span>Combine files<small>Merge PDFs and images into one</small></span>
            </button>
            <button className="home-action" onClick={async () => {
              const p = await openFileDialog({ multiple: true, filters: IMAGE_FILTER });
              if (p) openAny(Array.isArray(p) ? p : [p]);
            }}>
              <Images size={22} /><span>Create PDF from images<small>One page per image</small></span>
            </button>
          </div>
          <p className="drop-hint">Or drop files anywhere in this window.</p>
        </div>
        <div>
          <h2>Recent files</h2>
          {recent.length === 0 ? (
            <p className="muted">Files you open appear here.</p>
          ) : (
            <div className="recent">
              {recent.map((p) => (
                <button key={p} onClick={() => openPath(p)} title={p}>
                  <FileText size={20} style={{ color: "var(--accent)", flex: "none" }} />
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <div>{baseName(p)}</div>
                    <div className="path">{dirName(p)}</div>
                  </span>
                  <span className="ibtn small" role="button" aria-label="Remove from list" onClick={(e) => { e.stopPropagation(); forget(p); }}><X size={14} /></span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    </main>
  );
}
