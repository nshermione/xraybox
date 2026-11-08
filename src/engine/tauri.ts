import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type EngineStatus = "stopped" | "starting" | "running" | "stopping" | "failed";

export interface EngineSnapshot {
  status: EngineStatus;
  pid: number | null;
  lastError: string | null;
}

export interface VlessRealityProfile {
  remark: string;
  server: string;
  serverPort: number;
  uuid: string;
  serverName: string;
  publicKey: string;
  shortId: string;
  flow: string | null;
  fingerprint: string;
  spiderX: string | null;
  mldsa65Verify: string | null;
}

export const appendBootLog = (message: string) =>
  invoke<void>("append_boot_log", { message }).catch(() => undefined);

export const claimBootSplash = () => invoke<boolean>("claim_boot_splash");

export const connectAfterElevate = () => invoke<boolean>("connect_after_elevate");

export const startEngine = () => invoke<EngineSnapshot>("start_engine");
export const stopEngine = () => invoke<EngineSnapshot>("stop_engine");
export const engineStatus = () => invoke<EngineSnapshot>("engine_status");

export interface SessionTraffic {
  uplink: number;
  downlink: number;
  total: number;
  startedAtMs: number | null;
}

export interface PublicIpInfo {
  ip: string | null;
  countryCode: string | null;
}

export const sessionStats = () => invoke<SessionTraffic>("session_stats");
export const lookupPublicIp = () => invoke<PublicIpInfo>("lookup_public_ip");
export interface DnsSettings {
  remoteServers: string[];
  fallbackToSystem: boolean;
}

export const saveManualEndpoint = (profile: VlessRealityProfile) =>
  invoke<void>("save_manual_endpoint", { profile });
export const getManualEndpoint = () => invoke<VlessRealityProfile | null>("get_manual_endpoint");
export const getDnsSettings = () => invoke<DnsSettings>("get_dns_settings");
export const saveDnsSettings = (settings: DnsSettings) =>
  invoke<DnsSettings>("save_dns_settings", { settings });

export const updateGeoAssets = () => invoke<void>("update_geo_assets");

export type RuleAction = "direct" | "proxy" | "block";

export interface ServiceOverride {
  id: string;
  action: RuleAction;
}

export interface PackState {
  id: string;
  enabled: boolean;
  action: RuleAction;
  services: ServiceOverride[];
}

export const getRoutePacks = () => invoke<PackState[]>("get_route_packs");
export const saveRoutePacks = (packs: PackState[]) =>
  invoke<PackState[]>("save_route_packs", { packs });

export interface EngineSettings {
  autoConnect: boolean;
  startOnBoot: boolean;
  killSwitch: boolean;
  smartConnect: boolean;
  logsEnabled: boolean;
}

export type FetchInterval = "3h" | "1d" | "7d" | "off";

export interface SubscriptionSettings {
  url: string;
  fetchInterval: FetchInterval;
  lastFetchedAt: number | null;
  selectedProfileId: string | null;
  skippedCount: number;
}

export interface ProfileListItem {
  id: string;
  remark: string;
  server: string;
  serverPort: number;
  countryCode: string;
  latencyMs: number | null;
  profile: VlessRealityProfile;
}

export interface SubscriptionState {
  subscription: SubscriptionSettings;
  profiles: ProfileListItem[];
}

export const switchServer = (profile: VlessRealityProfile) =>
  invoke<EngineSnapshot>("switch_server", { profile });
export const getEngineSettings = () => invoke<EngineSettings>("get_engine_settings");
export const saveEngineSettings = (settings: EngineSettings) =>
  invoke<EngineSettings>("save_engine_settings", { settings });
export const getEngineLog = () => invoke<string>("get_engine_log");
export const getAppLog = () => invoke<string>("get_app_log");

export const saveSubscriptionUrl = (url: string) =>
  invoke<SubscriptionState>("save_subscription_url", { url });
export const getSubscription = () => invoke<SubscriptionState>("get_subscription");
export const fetchSubscription = () => invoke<SubscriptionState>("fetch_subscription");
export const listProfiles = () => invoke<ProfileListItem[]>("list_profiles");
export const selectProfile = (id: string) => invoke<SubscriptionState>("select_profile", { id });
export const pingProfiles = () => invoke<SubscriptionState>("ping_profiles");
export const clearProfiles = () => invoke<SubscriptionState>("clear_profiles");
export const setFetchInterval = (interval: FetchInterval) =>
  invoke<SubscriptionSettings>("set_fetch_interval", { interval });

export interface CommunityUsage {
  usedBytes: number;
  totalBytes: number;
  expiryAt: string | null;
}

export const getCommunityUsage = () => invoke<CommunityUsage | null>("get_community_usage");
export const verifyCommunity = () =>
  invoke<{ ok: boolean; expiresAt: string } | null>("verify_community");

export const APP_SHOWN_EVENT = "app-shown";
export const COMMUNITY_EVENT = "community-event";

export function listenAppShown(handler: () => void): Promise<UnlistenFn> {
  return listen(APP_SHOWN_EVENT, handler);
}

export function listenCommunityEvent(handler: (type: string) => void): Promise<UnlistenFn> {
  return listen<{ type: string }>(COMMUNITY_EVENT, (event) => {
    handler(event.payload.type);
  });
}
