import { reactive, ref } from "vue";
import {
  clearProfiles as clearProfilesCommand,
  fetchSubscription as fetchSubscriptionCommand,
  getSubscription,
  pingProfiles as pingProfilesCommand,
  saveSubscriptionUrl as saveSubscriptionUrlCommand,
  selectProfile as selectProfileCommand,
  setFetchInterval as setFetchIntervalCommand,
  type FetchInterval,
  type ProfileListItem,
  type SubscriptionSettings,
  type SubscriptionState,
  type VlessRealityProfile,
} from "../engine/tauri";
import { refreshCommunityUsage } from "./community";
import { t } from "./locale";
import { vpnState } from "./vpn";

export const FETCH_INTERVAL_IDS: FetchInterval[] = ["3h", "1d", "7d", "off"];
const RESUME_DEBOUNCE_SEC = 15 * 60;

export interface Server {
  id: string;
  name: string;
  region: string;
  countryCode: string;
  latencyMs: number | null;
  profile: VlessRealityProfile;
}

export const servers = ref<Server[]>([]);

export const subscriptionState = reactive({
  url: "",
  fetchInterval: "1d" as FetchInterval,
  lastFetchedAt: null as number | null,
  selectedProfileId: null as string | null,
  fetching: false,
  pinging: false,
  lastError: null as string | null,
});

export function applySubscriptionState(state: SubscriptionState): void {
  applySettings(state.subscription);
  servers.value = state.profiles.map(toServer);
  vpnState.selectedServerId = state.subscription.selectedProfileId ?? servers.value[0]?.id ?? "";
}

function applySettings(settings: SubscriptionSettings): void {
  subscriptionState.url = settings.url;
  subscriptionState.fetchInterval = settings.fetchInterval;
  subscriptionState.lastFetchedAt = settings.lastFetchedAt;
  subscriptionState.selectedProfileId = settings.selectedProfileId;
}

function toServer(item: ProfileListItem): Server {
  return {
    id: item.id,
    name: item.remark || item.server,
    region: item.server,
    countryCode: item.countryCode,
    latencyMs: item.latencyMs,
    profile: item.profile,
  };
}

export function selectedProfile(): VlessRealityProfile | null {
  return servers.value.find((server) => server.id === vpnState.selectedServerId)?.profile ?? null;
}

export async function hydrateSubscription(): Promise<void> {
  try {
    applySubscriptionState(await getSubscription());
  } catch {
    // Vite-only preview has no Tauri runtime.
  }
}

export async function saveSubscriptionUrl(url: string): Promise<void> {
  const trimmed = url.trim();
  if (!/^https?:\/\//i.test(trimmed)) {
    throw new Error(t("subscription.invalidUrl"));
  }
  subscriptionState.fetching = true;
  subscriptionState.lastError = null;
  try {
    applySubscriptionState(await saveSubscriptionUrlCommand(trimmed));
    void refreshCommunityUsage();
  } catch (error) {
    subscriptionState.lastError = errorMessage(error);
    throw error;
  } finally {
    subscriptionState.fetching = false;
  }
}

export async function refreshSubscription(): Promise<void> {
  if (!subscriptionState.url || subscriptionState.fetching) return;
  subscriptionState.fetching = true;
  subscriptionState.lastError = null;
  try {
    applySubscriptionState(await fetchSubscriptionCommand());
    void refreshCommunityUsage();
  } catch (error) {
    subscriptionState.lastError = errorMessage(error);
  } finally {
    subscriptionState.fetching = false;
  }
}

/** Network refresh at launch; call after `hydrateSubscription` has loaded the saved URL. */
export async function fetchOnAppOpen(): Promise<void> {
  if (subscriptionState.url) {
    await refreshSubscription();
  }
}

export async function fetchOnResume(): Promise<void> {
  if (!shouldFetchOnResume()) return;
  await refreshSubscription();
}

export function shouldFetchOnResume(): boolean {
  if (!subscriptionState.url || subscriptionState.fetchInterval === "off") return false;
  const last = subscriptionState.lastFetchedAt;
  if (last == null) return true;
  const now = Math.floor(Date.now() / 1000);
  if (now - last >= RESUME_DEBOUNCE_SEC) return true;
  return fetchIsDue(subscriptionState.fetchInterval, last, now);
}

export function fetchIsDue(interval: FetchInterval, lastFetchedAt: number | null, now: number): boolean {
  if (interval === "off") return false;
  if (lastFetchedAt == null) return true;
  const elapsed = now - lastFetchedAt;
  if (interval === "3h") return elapsed >= 3 * 3600;
  if (interval === "7d") return elapsed >= 7 * 24 * 3600;
  return elapsed >= 24 * 3600;
}

export async function persistSelectedProfile(id: string): Promise<void> {
  applySubscriptionState(await selectProfileCommand(id));
}

export async function refreshPings(): Promise<void> {
  if (subscriptionState.pinging || servers.value.length === 0) return;
  subscriptionState.pinging = true;
  try {
    applySubscriptionState(await pingProfilesCommand());
  } catch (error) {
    subscriptionState.lastError = errorMessage(error);
  } finally {
    subscriptionState.pinging = false;
  }
}

export async function clearAllProfiles(): Promise<void> {
  subscriptionState.lastError = null;
  try {
    applySubscriptionState(await clearProfilesCommand());
  } catch (error) {
    subscriptionState.lastError = errorMessage(error);
    throw error;
  }
}

export async function persistFetchInterval(interval: FetchInterval): Promise<void> {
  applySettings(await setFetchIntervalCommand(interval));
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  return String(error);
}
