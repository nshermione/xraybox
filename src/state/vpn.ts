import { computed, reactive, ref } from "vue";
import {
  appendBootLog,
  engineStatus,
  lookupPublicIp,
  sessionStats,
  startEngine,
  stopEngine,
  switchServer,
  type EngineSnapshot,
  type EngineStatus,
  type VlessRealityProfile,
} from "../engine/tauri";
import { countryName } from "../geo/country";
import { isIos, isMac } from "../platform";
import { t } from "./locale";
import { engineSettings } from "./engineSettings";
import {
  hydrateSubscription,
  persistSelectedProfile,
  selectedProfile,
  servers,
  type Server,
} from "./subscription";

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "disconnecting";
export type ProxyMode = "proxy" | "tun";
export type { Server };

const RECENT_LIMIT = 3;
const RECENT_STORAGE_KEY = "xraybox.recentServers";

export const vpnState = reactive({
  status: "disconnected" as ConnectionStatus,
  mode: "proxy" as ProxyMode,
  selectedServerId: "",
  recentServerIds: readRecentIds(),
  lastError: null as string | null,
  manualRemark: null as string | null,
  reloading: false,
});

export const sessionState = reactive({
  startedAt: null as number | null,
  bytes: null as number | null,
  ip: "",
  countryCode: "",
});

export const sessionNow = ref(Date.now());

const STATS_POLL_MS = 2000;
let statsTimer = 0;
let durationTimer = 0;

export const selectedServer = computed(() => {
  const fallback = emptyServer();
  return servers.value.find((server) => server.id === vpnState.selectedServerId) ?? servers.value[0] ?? fallback;
});

export function pickSmartServer(list: readonly Server[]): Server | undefined {
  let best: Server | undefined;
  for (const server of list) {
    if (server.latencyMs == null) continue;
    if (best == null || server.latencyMs < (best.latencyMs ?? Number.POSITIVE_INFINITY)) {
      best = server;
    }
  }
  return best;
}

const sessionStartedWithSmartConnect = ref(false);
let smartSessionLocked = false;

function isLiveSession(status: ConnectionStatus): boolean {
  return status === "connected" || status === "connecting" || status === "disconnecting";
}

function lockSmartSession(active: boolean): void {
  sessionStartedWithSmartConnect.value = active;
  smartSessionLocked = true;
}

function clearSmartSession(): void {
  sessionStartedWithSmartConnect.value = false;
  smartSessionLocked = false;
}

function armSmartSession(): void {
  lockSmartSession(engineSettings.smartConnect);
}

export const smartConnectActive = computed(() => {
  if (!engineSettings.smartConnect) return false;
  if (isLiveSession(vpnState.status)) return sessionStartedWithSmartConnect.value;
  return true;
});

export const displayedServer = computed(() => {
  const selected = selectedServer.value;
  if (!smartConnectActive.value) return selected;
  const server =
    vpnState.status === "connected" ? selected : (pickSmartServer(servers.value) ?? selected);
  const country = countryName(server.countryCode);
  return {
    ...server,
    name: t("vpn.smartConnect"),
    region: country || (server.id ? server.name : t("servers.emptyTitle")),
  };
});

export const recentServers = computed(() =>
  vpnState.recentServerIds
    .map((id) => servers.value.find((server) => server.id === id))
    .filter((server): server is Server => server != null)
    .slice(0, RECENT_LIMIT),
);

let engineCallInFlight = false;
let cancelRequested = false;

let markVpnStatusKnown: () => void = () => undefined;
const vpnStatusKnown = new Promise<void>((resolve) => {
  markVpnStatusKnown = resolve;
});

/** Resolves once the engine status was read (or failed), so the UI can avoid showing a stale "disconnected". */
export function whenVpnStatusKnown(): Promise<void> {
  return vpnStatusKnown;
}

export async function hydrateVpnStatus(): Promise<void> {
  try {
    await readEngineStatus();
  } finally {
    markVpnStatusKnown();
  }
}

async function readEngineStatus(): Promise<void> {
  const started = performance.now();
  console.log("[xraybox-boot] engineStatus invoke");
  void appendBootLog("engineStatus invoke");
  try {
    const snapshot = await engineStatus();
    const text = `engineStatus ok ${Math.round(performance.now() - started)}ms ${JSON.stringify(snapshot)}`;
    console.log(`[xraybox-boot] ${text}`);
    void appendBootLog(text);
    applySnapshot(snapshot);
    if (vpnState.status !== "connected") {
      void refreshPublicIp();
    }
  } catch (error) {
    const text = `engineStatus fail ${Math.round(performance.now() - started)}ms ${
      error instanceof Error ? error.message : String(error)
    }`;
    console.log(`[xraybox-boot] ${text}`);
    void appendBootLog(text);
    void refreshPublicIp();
  }
}

export async function selectServer(id: string): Promise<void> {
  vpnState.selectedServerId = id;
  const next = [id, ...vpnState.recentServerIds.filter((recentId) => recentId !== id)].slice(0, RECENT_LIMIT);
  vpnState.recentServerIds = next;
  persistRecentIds(next);
  await persistSelectedProfile(id);
}

export async function reconnect(): Promise<void> {
  if (engineCallInFlight) return;
  engineCallInFlight = true;
  try {
    if (vpnState.status === "connected") {
      const profile = selectedProfile();
      if (!profile) {
        throw new Error(t("vpn.noProfile"));
      }
      vpnState.reloading = true;
      applySnapshot(await switchServer(profile));
      resetCurrentSession();
      return;
    }
    if (vpnState.status === "connecting" || vpnState.status === "disconnecting") {
      vpnState.status = "disconnecting";
      try {
        applySnapshot(await stopEngine());
      } catch (error) {
        setDisplayedError(errorMessage(error), error);
      }
    }
    armSmartSession();
    vpnState.status = "connecting";
    vpnState.lastError = null;
    applySnapshot(await startEngine());
    await hydrateSubscription();
  } catch (error) {
    markDisconnected();
    setDisplayedError(errorMessage(error), error);
  } finally {
    vpnState.reloading = false;
    engineCallInFlight = false;
  }
}

export function setMode(mode: ProxyMode): void {
  vpnState.mode = mode;
}

export async function toggleConnect(): Promise<void> {
  if (engineCallInFlight && vpnState.status === "connecting") {
    cancelRequested = true;
    vpnState.status = "disconnecting";
    vpnState.lastError = null;
    try {
      applySnapshot(await stopEngine());
    } catch {
      // start() still owns teardown if stop races the in-flight start
    }
    return;
  }

  if (engineCallInFlight) return;
  engineCallInFlight = true;
  try {
    if (
      vpnState.status === "connected" ||
      vpnState.status === "connecting" ||
      vpnState.status === "disconnecting"
    ) {
      vpnState.status = "disconnecting";
      applySnapshot(await stopEngine());
      return;
    }

    armSmartSession();
    vpnState.status = "connecting";
    vpnState.lastError = null;
    const snapshot = await startEngine();
    if (cancelRequested) {
      applySnapshot(await stopEngine());
      return;
    }
    applySnapshot(snapshot);
    await hydrateSubscription();
  } catch (error) {
    if (cancelRequested) {
      markDisconnected();
      vpnState.lastError = null;
      return;
    }
    markDisconnected();
    setDisplayedError(errorMessage(error), error);
  } finally {
    engineCallInFlight = false;
    cancelRequested = false;
  }
}

function markDisconnected(): void {
  const previous = vpnState.status;
  vpnState.status = "disconnected";
  syncSessionLifecycle(previous, "disconnected");
}

function applySnapshot(snapshot: EngineSnapshot): void {
  const previous = vpnState.status;
  const next = mapEngineStatus(snapshot.status);
  vpnState.status = next;
  if (snapshot.lastError) {
    setDisplayedError(displayEngineError(snapshot.lastError), snapshot.lastError);
  } else {
    vpnState.lastError = null;
  }
  syncSessionLifecycle(previous, next);
}

function syncSessionLifecycle(previous: ConnectionStatus, next: ConnectionStatus): void {
  const becameConnected = next === "connected" && previous !== "connected";
  const leftConnected = next !== "connected" && previous === "connected";
  const becameDisconnected = next === "disconnected" && previous !== "disconnected";
  if (becameConnected) {
    if (!smartSessionLocked) {
      lockSmartSession(engineSettings.smartConnect);
    }
    startSessionPolling();
    return;
  }
  if (leftConnected) {
    stopSessionPolling();
    sessionState.startedAt = null;
    sessionState.bytes = null;
    invalidatePublicIp();
  }
  if (becameDisconnected) {
    clearSmartSession();
    void refreshPublicIp();
  }
}

function resetCurrentSession(): void {
  sessionState.startedAt = Date.now();
  sessionState.bytes = 0;
  void refreshSessionStats();
  void refreshPublicIp();
}

function startSessionPolling(): void {
  stopSessionPolling();
  sessionNow.value = Date.now();
  durationTimer = window.setInterval(() => {
    sessionNow.value = Date.now();
  }, 1000);
  void refreshSessionStats();
  void refreshPublicIp();
  statsTimer = window.setInterval(() => {
    void refreshSessionStats();
  }, STATS_POLL_MS);
}

function stopSessionPolling(): void {
  window.clearInterval(statsTimer);
  window.clearInterval(durationTimer);
  statsTimer = 0;
  durationTimer = 0;
}

export async function refreshSessionStats(): Promise<void> {
  if (vpnState.status !== "connected") {
    return;
  }
  try {
    const snapshot = await sessionStats();
    sessionState.bytes = snapshot.total;
    if (snapshot.startedAtMs) {
      sessionState.startedAt = snapshot.startedAtMs;
    } else if (sessionState.startedAt == null) {
      sessionState.startedAt = Date.now();
    }
  } catch (error) {
    console.error("[xraybox] session stats failed", error);
    sessionState.bytes = null;
  }
}

let publicIpLookupId = 0;

function invalidatePublicIp(): void {
  publicIpLookupId += 1;
  sessionState.ip = "";
  sessionState.countryCode = "";
}

export async function refreshPublicIp(): Promise<void> {
  const requestId = ++publicIpLookupId;
  try {
    const result = await lookupPublicIp();
    if (requestId !== publicIpLookupId) return;
    sessionState.ip = result.ip ?? "";
    sessionState.countryCode = result.countryCode ?? "";
  } catch (error) {
    if (requestId !== publicIpLookupId) return;
    console.error("[xraybox] public ip lookup failed", error);
    sessionState.ip = "";
    sessionState.countryCode = "";
  }
}

function setDisplayedError(shown: string, raw: unknown): void {
  vpnState.lastError = shown;
  const rawText = raw instanceof Error ? raw.message : typeof raw === "string" ? raw : String(raw);
  console.error("[xraybox] connect error", shown, rawText);
}

function mapEngineStatus(status: EngineStatus): ConnectionStatus {
  switch (status) {
    case "running":
      return "connected";
    case "starting":
      return "connecting";
    case "stopping":
      return "disconnecting";
    default:
      return "disconnected";
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return displayEngineError(error.message);
  if (typeof error === "string") return displayEngineError(error);
  return "Engine command failed";
}

export async function applyConnectedProfile(profile: VlessRealityProfile): Promise<void> {
  if (vpnState.status !== "connected") return;
  vpnState.reloading = true;
  try {
    applySnapshot(await switchServer(profile));
    resetCurrentSession();
  } catch (error) {
    markDisconnected();
    setDisplayedError(errorMessage(error), error);
    throw error;
  } finally {
    vpnState.reloading = false;
  }
}

function displayEngineError(message: string): string {
  const known: Record<string, string> = {
    need_admin: isIos
      ? "vpn.errorNeedAdminIos"
      : isMac
        ? "vpn.errorNeedAdminMac"
        : "vpn.errorNeedAdmin",
    uac_cancelled: isIos
      ? "vpn.errorVpnDeniedIos"
      : isMac
        ? "vpn.errorUacCancelledMac"
        : "vpn.errorUacCancelled",
    tunnel_no_response: "vpn.errorTunnelNoResponse",
    tunnel_refused: "vpn.errorTunnelRefused",
    tunnel_unreachable: "vpn.errorTunnelUnreachable",
    tunnel_hijacked: "vpn.errorTunnelHijacked",
    reality_invalid_connection: "vpn.errorRealityFailed",
    wintun_dll_missing: "vpn.errorWintunMissing",
    vpn_denied: isIos ? "vpn.errorVpnDeniedIos" : "vpn.errorVpnDenied",
    ios_memory_budget: "vpn.errorIosMemory",
    xray_sidecar_missing: "vpn.errorSidecarMissing",
    vpn_no_underlying_iface: "vpn.errorNoUnderlyingIface",
    helper_install_failed: "vpn.errorHelperInstallFailed",
    helper_unavailable: "vpn.errorHelperUnavailable",
  };
  const stripped = message.replace(/^process error:\s*/i, "");
  for (const [code, key] of Object.entries(known)) {
    if (stripped === code || stripped.startsWith(`${code}:`) || stripped.startsWith(`${code} `)) {
      return t(key);
    }
  }
  return message;
}

function emptyServer(): Server {
  return {
    id: "",
    name: vpnState.manualRemark ?? t("servers.emptyTitle"),
    region: "",
    countryCode: "xx",
    latencyMs: null,
    profile: {
      remark: "",
      server: "",
      serverPort: 443,
      uuid: "",
      serverName: "",
      publicKey: "",
      shortId: "",
      flow: null,
      fingerprint: "chrome",
      spiderX: null,
      mldsa65Verify: null,
    },
  };
}

function readRecentIds(): string[] {
  try {
    const stored = localStorage.getItem(RECENT_STORAGE_KEY);
    if (!stored) return [];
    const parsed: unknown = JSON.parse(stored);
    return Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === "string") : [];
  } catch {
    return [];
  }
}

function persistRecentIds(ids: string[]): void {
  localStorage.setItem(RECENT_STORAGE_KEY, JSON.stringify(ids));
}
