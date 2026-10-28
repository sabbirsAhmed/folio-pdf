import { api } from "./api";

/** LRU cache of rendered page bitmaps as object URLs. */
const cache = new Map<string, string>();
const inflight = new Map<string, Promise<string>>();
const MAX = 90;

// Rendering runs on a single engine thread, so keep a small queue and
// let the most recently requested (visible) pages go first.
type Job = { key: string; run: () => Promise<void> };
const queue: Job[] = [];
let running = 0;
const CONCURRENCY = 2;

function pump() {
  while (running < CONCURRENCY && queue.length) {
    const job = queue.pop()!;
    running++;
    job.run().finally(() => {
      running--;
      pump();
    });
  }
}

export function renderKey(id: number, version: number, page: number, scale: number) {
  return `${id}:${version}:${page}:${scale}`;
}

export function cached(key: string) {
  const v = cache.get(key);
  if (v) {
    cache.delete(key);
    cache.set(key, v);
  }
  return v;
}

export function requestRender(id: number, version: number, page: number, scale: number): Promise<string> {
  const key = renderKey(id, version, page, scale);
  const hit = cached(key);
  if (hit) return Promise.resolve(hit);
  const pending = inflight.get(key);
  if (pending) return pending;
  const p = new Promise<string>((resolve, reject) => {
    queue.push({
      key,
      run: async () => {
        try {
          const blob = await api.renderPage(id, page, scale);
          const url = URL.createObjectURL(blob);
          cache.set(key, url);
          while (cache.size > MAX) {
            const [k, u] = cache.entries().next().value as [string, string];
            cache.delete(k);
            URL.revokeObjectURL(u);
          }
          resolve(url);
        } catch (e) {
          reject(e);
        } finally {
          inflight.delete(key);
        }
      },
    });
    pump();
  });
  inflight.set(key, p);
  return p;
}

/** Drop queued renders that are no longer wanted (e.g. after zoom change). */
export function cancelQueued(predicate: (key: string) => boolean) {
  for (let i = queue.length - 1; i >= 0; i--) {
    if (predicate(queue[i].key)) queue.splice(i, 1);
  }
}

export function clearDocCache(id: number) {
  for (const [k, u] of [...cache.entries()]) {
    if (k.startsWith(`${id}:`)) {
      cache.delete(k);
      URL.revokeObjectURL(u);
    }
  }
}
