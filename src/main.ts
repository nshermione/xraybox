import { createApp } from "vue";
import App from "./App.vue";
import { runBootSplash, whenAppPainted } from "./boot/splash";
import { scheduleIdleFlagPreload } from "./geo/flags";
import { initLocale } from "./state/locale";
import { initTheme } from "./state/theme";
import { hydrateDnsSettings } from "./state/dns";
import { engineSettings, hydrateEngineSettings } from "./state/engineSettings";
import { hydrateRoutePacks } from "./state/packs";
import { hydrateManualEndpoint } from "./state/endpoint";
import { fetchOnAppOpen, hydrateSubscription, servers } from "./state/subscription";
import { hydrateVpnStatus, toggleConnect, vpnState } from "./state/vpn";
import { appendBootLog, connectAfterElevate } from "./engine/tauri";
import "./styles/tokens.css";

function boot(step: string, extra?: unknown): void {
  const text = extra === undefined ? step : `${step} ${stringify(extra)}`;
  console.log(`[xraybox-boot] ${text}`);
  void appendBootLog(text);
}

function stringify(value: unknown): string {
  if (value instanceof Error) return value.message;
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

async function timed(step: string, work: () => Promise<void>): Promise<void> {
  const started = performance.now();
  boot(`${step} start`);
  try {
    await work();
    boot(`${step} done ${Math.round(performance.now() - started)}ms`);
  } catch (error) {
    boot(`${step} fail ${Math.round(performance.now() - started)}ms`, error);
  }
}

boot("main.ts running");
initLocale();
initTheme();
boot("locale+theme done");
void hydrateThenMaybeConnect();
boot("mounting Vue");
createApp(App).mount("#app");
boot("Vue mounted");
void runBootSplash().then(() => {
  boot("splash finished");
  scheduleIdleFlagPreload(servers.value.map((server) => server.countryCode));
});

async function hydrateThenMaybeConnect(): Promise<void> {
  await Promise.all([
    timed("hydrateManualEndpoint", hydrateManualEndpoint),
    timed("hydrateDnsSettings", hydrateDnsSettings),
    timed("hydrateEngineSettings", hydrateEngineSettings),
    timed("hydrateRoutePacks", hydrateRoutePacks),
    timed("hydrateSubscription", hydrateSubscription),
  ]);
  // The network refresh can take seconds; the connection status must not wait for it.
  const subscriptionRefresh = timed("refreshSubscription", fetchOnAppOpen);
  await timed("hydrateVpnStatus", hydrateVpnStatus);
  await subscriptionRefresh;
  const pendingConnect = await connectAfterElevate().catch(() => false);
  if ((engineSettings.autoConnect || pendingConnect) && vpnState.status === "disconnected") {
    await Promise.race([whenAppPainted(), new Promise<void>((resolve) => window.setTimeout(resolve, 3000))]);
    if (vpnState.status !== "disconnected") return;
    boot(pendingConnect ? "connect-after-elevate start" : "auto-connect start");
    void toggleConnect();
  }
}
