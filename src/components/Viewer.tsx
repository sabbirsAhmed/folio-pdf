import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useStore } from "../store";
import { PageView } from "./PageView";
import { zoomBy } from "../actions";

export function Viewer() {
  const t = useStore((s) => (s.active != null ? s.tabs[s.active] : null))!;
  const tool = useStore((s) => s.tool);
  const patchTab = useStore((s) => s.patchTab);
  const scrollRequest = useStore((s) => s.scrollRequest);
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [visible, setVisible] = useState<Set<number>>(new Set([0, 1]));
  const pageRefs = useRef<(HTMLDivElement | null)[]>([]);
  pageRefs.current.length = t.info.page_count;
  const anchor = useRef<{ page: number; frac: number } | null>(null);
  const drag = useRef<{ x: number; y: number; sl: number; st: number } | null>(null);
  const [dragging, setDragging] = useState(false);

  // Effective zoom from fit mode.
  const maxW = Math.max(...t.info.pages.map((p) => p.width), 1);
  const first = t.info.pages[t.page] ?? t.info.pages[0];
  let z = t.zoom;
  if (t.fit === "width") z = Math.max(0.1, (size.w - 56) / maxW);
  if (t.fit === "page" && first) z = Math.max(0.1, Math.min((size.w - 56) / first.width, (size.h - 36) / first.height));
  z = Math.round(z * 1000) / 1000;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    setSize({ w: el.clientWidth, h: el.clientHeight });
    return () => ro.disconnect();
  }, []);

  // Track which pages are near the viewport and which one is current.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        setVisible((prev) => {
          const next = new Set(prev);
          for (const e of entries) {
            const i = Number((e.target as HTMLElement).dataset.page);
            if (e.isIntersecting) next.add(i);
            else next.delete(i);
          }
          return next;
        });
      },
      { root: el, rootMargin: "600px 0px" },
    );
    pageRefs.current.forEach((p) => p && io.observe(p));
    return () => io.disconnect();
  }, [t.info.page_count, t.version, z]);

  const onScroll = () => {
    const el = ref.current;
    if (!el) return;
    const mid = el.scrollTop + el.clientHeight * 0.35;
    let cur = 0;
    for (let i = 0; i < pageRefs.current.length; i++) {
      const p = pageRefs.current[i];
      if (p && p.offsetTop <= mid) cur = i;
    }
    const p = pageRefs.current[cur];
    if (p) anchor.current = { page: cur, frac: (el.scrollTop - p.offsetTop) / Math.max(1, p.offsetHeight) };
    const live = useStore.getState().tabs[t.info.id];
    if (live && live.page !== cur) patchTab(t.info.id, { page: cur });
  };

  // Keep the reading position stable across zoom changes.
  useLayoutEffect(() => {
    const el = ref.current;
    const a = anchor.current;
    if (!el || !a) return;
    const p = pageRefs.current[a.page];
    if (p) el.scrollTop = p.offsetTop + a.frac * p.offsetHeight;
  }, [z]);

  // Programmatic navigation.
  useEffect(() => {
    if (!scrollRequest || scrollRequest.id !== t.info.id) return;
    const el = ref.current;
    const p = pageRefs.current[scrollRequest.page];
    if (!el || !p) return;
    const y = scrollRequest.y != null ? p.offsetTop + scrollRequest.y * z - el.clientHeight * 0.3 : p.offsetTop - 12;
    el.scrollTo({ top: Math.max(0, y) });
  }, [scrollRequest]);

  // Ctrl + wheel zoom.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      zoomBy(e.deltaY < 0 ? 1.1 : 1 / 1.1);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  const hand = tool === "hand";
  return (
    <div
      ref={ref}
      className={`viewer ${hand ? "hand" : ""} ${dragging ? "dragging" : ""}`}
      onScroll={onScroll}
      onPointerDown={(e) => {
        if (!hand && e.button !== 1) return;
        const el = ref.current!;
        drag.current = { x: e.clientX, y: e.clientY, sl: el.scrollLeft, st: el.scrollTop };
        setDragging(true);
        (e.target as Element).setPointerCapture?.(e.pointerId);
        e.preventDefault();
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const el = ref.current!;
        el.scrollLeft = d.sl - (e.clientX - d.x);
        el.scrollTop = d.st - (e.clientY - d.y);
      }}
      onPointerUp={() => { drag.current = null; setDragging(false); }}
    >
      <div className="pages">
        {t.info.pages.map((p, i) => (
          <div key={i} data-page={i} ref={(el) => (pageRefs.current[i] = el)}>
            <PageView tab={t} page={p} z={z} visible={visible.has(i)} />
          </div>
        ))}
      </div>
    </div>
  );
}
