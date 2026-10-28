import { useEffect, useRef, useState } from "react";
import { requestRender } from "../render";
import type { PageInfo } from "../api";

/** Lazily rendered page thumbnail sized to `width` CSS pixels. */
export function Thumb({ id, version, page, width }: { id: number; version: number; page: PageInfo; width: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [url, setUrl] = useState<string | null>(null);
  const height = (width * page.height) / page.width;
  const scale = Math.round(((width * (window.devicePixelRatio || 1)) / page.width) * 20) / 20;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver((e) => setVisible(e.some((x) => x.isIntersecting)), { rootMargin: "300px" });
    io.observe(el);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    if (!visible) return;
    let live = true;
    requestRender(id, version, page.index, Math.max(0.05, scale))
      .then((u) => live && setUrl(u))
      .catch(() => undefined);
    return () => { live = false; };
  }, [visible, id, version, page.index, scale]);

  return (
    <div ref={ref} className="frame" style={{ width, height }}>
      {url && <img src={url} alt={`Page ${page.index + 1}`} draggable={false} />}
    </div>
  );
}
