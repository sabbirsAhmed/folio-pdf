import { useStore } from "../store";

export function StatusBar() {
  const busy = useStore((s) => s.busy);
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null));
  const p = t ? t.info.pages[t.page] : null;
  return (
    <div className="status">
      {busy ? (
        <span style={{ display: "inline-flex", gap: 8, alignItems: "center" }}>
          <span className="spinner" /> {busy}…
        </span>
      ) : (
        <span>{t ? (t.info.dirty ? "Unsaved changes" : t.info.path ? "Saved" : "Not saved yet") : "Ready"}</span>
      )}
      <span className="grow" />
      {t && p && (
        <>
          <span>{(p.width / 72 * 25.4).toFixed(0)} × {(p.height / 72 * 25.4).toFixed(0)} mm</span>
          <span>{t.info.protected ? "Password protected" : ""}</span>
          <span>PDF {t.info.version.replace("Pdf", "").replace(/_/g, ".")}</span>
        </>
      )}
    </div>
  );
}

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  const dismiss = useStore((s) => s.dismiss);
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`}>
          <span>{t.text}</span>
          {t.action && (
            <button onClick={() => { t.action!.run(); dismiss(t.id); }}>{t.action.label}</button>
          )}
          <button onClick={() => dismiss(t.id)} aria-label="Dismiss">✕</button>
        </div>
      ))}
    </div>
  );
}
