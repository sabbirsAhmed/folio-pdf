import { House, X } from "lucide-react";
import { useStore } from "../store";
import { closeDoc } from "../actions";

export function TabStrip() {
  const order = useStore((s) => s.order);
  const tabs = useStore((s) => s.tabs);
  const active = useStore((s) => s.active);
  const setActive = useStore((s) => s.setActive);
  return (
    <div className="tabstrip" role="tablist">
      <button className={`ibtn home-btn ${active == null ? "on" : ""}`} title="Home" onClick={() => setActive(null)}>
        <House size={16} />
      </button>
      {order.map((id) => {
        const t = tabs[id];
        if (!t) return null;
        return (
          <div
            key={id}
            role="tab"
            aria-selected={active === id}
            className={`tab ${active === id ? "active" : ""}`}
            title={t.info.path ?? t.info.name}
            onMouseDown={(e) => {
              if (e.button === 1) { e.preventDefault(); closeDoc(id); } else setActive(id);
            }}
          >
            {t.info.dirty && <span className="dot" title="Unsaved changes" />}
            <span className="name">{t.info.name}</span>
            <button className="ibtn small x" aria-label={`Close ${t.info.name}`} onMouseDown={(e) => e.stopPropagation()} onClick={() => closeDoc(id)}>
              <X size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
