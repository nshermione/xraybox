import { appendBootLog, claimBootSplash } from "../engine/tauri";
import { whenVpnStatusKnown } from "../state/vpn";

const SPLASH_SEEN_KEY = "xraybox.splashSeen";
const WARM_STATUS_WAIT_MS = 1500;
const SPIN_MIN_MS = 80;
const SETTLE_MS = 200;
const MORPH_MS = 200;
const WORD_HOLD_MS = 800;
const LEAVE_MS = 200;
const READY_TIMEOUT_MS = 8000;
const FORCE_REVEAL_MS = 12000;
const C_ANGLE_DEG = 225;

let bootStarted = false;
let painted = false;
let resolvePainted: (() => void) | undefined;

const appPainted = new Promise<void>((resolve) => {
  resolvePainted = resolve;
});

export function whenAppPainted(): Promise<void> {
  return appPainted;
}

export function markAppPainted(): void {
  if (painted) return;
  painted = true;
  console.log("[xraybox-boot] markAppPainted");
  void appendBootLog("markAppPainted");
  requestAnimationFrame(() => {
    requestAnimationFrame(() => resolvePainted?.());
  });
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms);
  });
}

function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function readRotationDeg(element: Element): number {
  const { transform } = getComputedStyle(element);
  if (!transform || transform === "none") return 0;

  const match = /matrix\(([-0-9.e]+),\s*([-0-9.e]+)/.exec(transform);
  if (match) {
    const angle = Math.atan2(Number(match[2]), Number(match[1])) * (180 / Math.PI);
    return angle < 0 ? angle + 360 : angle;
  }

  try {
    const matrix = new DOMMatrix(transform);
    const angle = Math.atan2(matrix.b, matrix.a) * (180 / Math.PI);
    return angle < 0 ? angle + 360 : angle;
  } catch {
    return 0;
  }
}

function settleRing(splash: HTMLElement): void {
  const spinner = splash.querySelector(".boot-spinner");
  if (!(spinner instanceof SVGElement) && !(spinner instanceof HTMLElement)) return;

  const current = readRotationDeg(spinner);
  spinner.style.animation = "none";
  spinner.style.transform = `rotate(${current}deg)`;
  void spinner.getBoundingClientRect();

  let target = C_ANGLE_DEG;
  while (target < current + 12) target += 360;

  spinner.style.transition = "transform 420ms cubic-bezier(0.22, 1, 0.36, 1)";
  spinner.style.transform = `rotate(${target}deg)`;
}

async function waitForAppReady(): Promise<void> {
  const fontsReady = document.fonts?.ready.catch(() => undefined) ?? Promise.resolve();

  await Promise.race([
    Promise.all([appPainted, fontsReady, whenVpnStatusKnown()]),
    delay(READY_TIMEOUT_MS),
  ]);
}

function revealApp(splash: HTMLElement | null, app: HTMLElement): void {
  console.log("[xraybox-boot] splash reveal");
  void appendBootLog("splash reveal");
  splash?.setAttribute("data-phase", "leave");
  app.classList.add("is-revealed");
  rememberSplashSeen();
}

async function isFirstLoadOfProcess(): Promise<boolean> {
  try {
    return await claimBootSplash();
  } catch {
    // No Tauri runtime (Vite-only preview): behave like a fresh launch.
    return true;
  }
}

function rememberSplashSeen(): void {
  try {
    sessionStorage.setItem(SPLASH_SEEN_KEY, "1");
  } catch {
    // Storage blocked: the Rust claim still covers webview reloads.
  }
}

async function showWarmApp(splash: HTMLElement, app: HTMLElement): Promise<void> {
  // Keep #app hidden over the splash background until the real connection
  // status is known, otherwise it flashes "disconnected" before "connected".
  await Promise.race([whenVpnStatusKnown(), delay(WARM_STATUS_WAIT_MS)]);
  app.style.transition = "none";
  app.classList.add("is-revealed");
  splash.remove();
  rememberSplashSeen();
}

export async function runBootSplash(): Promise<void> {
  if (bootStarted) return;
  bootStarted = true;
  console.log("[xraybox-boot] splash start");
  void appendBootLog("splash start");

  const splash = document.getElementById("boot-splash");
  const app = document.getElementById("app");
  if (!app) {
    console.log("[xraybox-boot] splash abort: #app missing");
    return;
  }

  if (!splash) {
    app.classList.add("is-revealed");
    return;
  }

  const firstLoadOfProcess = await isFirstLoadOfProcess();
  if (!firstLoadOfProcess || document.documentElement.dataset.boot === "warm") {
    console.log("[xraybox-boot] splash skipped: webview reloaded in a live process");
    void appendBootLog("splash skipped");
    await showWarmApp(splash, app);
    return;
  }

  const forceTimer = window.setTimeout(() => {
    if (!document.getElementById("boot-splash")) return;
    revealApp(splash, app);
    splash.remove();
  }, FORCE_REVEAL_MS);

  const startedAt = performance.now();
  await waitForAppReady();

  if (prefersReducedMotion()) {
    splash.setAttribute("data-phase", "word");
    await delay(360);
    revealApp(splash, app);
    await delay(LEAVE_MS);
    splash.remove();
    window.clearTimeout(forceTimer);
    return;
  }

  const remainingSpin = Math.max(0, SPIN_MIN_MS - (performance.now() - startedAt));
  await delay(remainingSpin);

  splash.setAttribute("data-phase", "settle");
  settleRing(splash);
  await delay(SETTLE_MS);

  splash.setAttribute("data-phase", "word");
  await delay(MORPH_MS + WORD_HOLD_MS);

  revealApp(splash, app);
  await delay(LEAVE_MS);
  splash.remove();
  window.clearTimeout(forceTimer);
}
