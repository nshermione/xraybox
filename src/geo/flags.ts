import { normalizeCountryCode } from "./country";

const flagLoaders = import.meta.glob<string>("../../node_modules/flag-icons/flags/4x3/*.svg", {
  // Raw SVG → blob URL. Vite ?url is a JS module in dev and may keep
  // ?url&no-inline on the path; WebView2 then refuses to render it as an image.
  // Data URIs also break flag-icons url(#clipPath) / xlink:href fragments.
  query: "?raw",
  import: "default",
});

const loadersByCode = new Map<string, () => Promise<string>>();

for (const [path, load] of Object.entries(flagLoaders)) {
  const file = path.replace(/\\/g, "/").split("/").pop();
  if (!file) continue;
  loadersByCode.set(file.replace(/\.svg$/i, ""), load);
}

const urlCache = new Map<string, string>();
const inflight = new Map<string, Promise<string>>();

function loaderFor(code: string): (() => Promise<string>) | undefined {
  return loadersByCode.get(code) ?? loadersByCode.get("xx");
}

function svgToObjectUrl(svg: string): string {
  return URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
}

export function peekFlag(code: string): string {
  return urlCache.get(normalizeCountryCode(code)) ?? "";
}

export function loadFlag(code: string): Promise<string> {
  const iso = normalizeCountryCode(code);
  const cached = urlCache.get(iso);
  if (cached) return Promise.resolve(cached);

  const pending = inflight.get(iso);
  if (pending) return pending;

  const load = loaderFor(iso);
  if (!load) return Promise.resolve("");

  const request = load()
    .then((svg) => {
      const url = svgToObjectUrl(svg);
      const previous = urlCache.get(iso);
      urlCache.set(iso, url);
      if (previous && previous !== url) URL.revokeObjectURL(previous);
      return url;
    })
    .catch(() => {
      if (iso === "xx") return "";
      return loadFlag("xx");
    })
    .finally(() => {
      inflight.delete(iso);
    });

  inflight.set(iso, request);
  return request;
}

export function scheduleIdleFlagPreload(codes: readonly string[]): void {
  const unique = [...new Set(codes.map(normalizeCountryCode))].filter((code) => !urlCache.has(code));
  if (unique.length === 0) return;

  const run = () => {
    void Promise.all(unique.map((code) => loadFlag(code)));
  };

  if (typeof window.requestIdleCallback === "function") {
    window.requestIdleCallback(run, { timeout: 2500 });
    return;
  }

  window.setTimeout(run, 200);
}

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    for (const url of urlCache.values()) URL.revokeObjectURL(url);
    urlCache.clear();
    inflight.clear();
  });
}
