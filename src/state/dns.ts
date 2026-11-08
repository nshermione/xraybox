import { computed, reactive } from "vue";
import { getDnsSettings, saveDnsSettings, type DnsSettings } from "../engine/tauri";
import { vpnState } from "./vpn";

export const dnsState = reactive<DnsSettings>({
  remoteServers: ["1.1.1.1", "8.8.8.8"],
  fallbackToSystem: false,
});

export const dnsEditorsLocked = computed(() => vpnState.status !== "disconnected");

export const remoteDnsSummary = computed(() => dnsState.remoteServers.join(", "));

export async function hydrateDnsSettings(): Promise<void> {
  try {
    const settings = await getDnsSettings();
    dnsState.remoteServers = settings.remoteServers;
    dnsState.fallbackToSystem = Boolean(settings.fallbackToSystem);
  } catch {
    // Vite-only preview has no Tauri runtime.
  }
}

export async function setRemoteServers(servers: string[]): Promise<void> {
  if (dnsEditorsLocked.value) return;
  const next = {
    remoteServers: servers,
    fallbackToSystem: dnsState.fallbackToSystem,
  };
  const saved = await saveDnsSettings(next);
  dnsState.remoteServers = saved.remoteServers;
  dnsState.fallbackToSystem = Boolean(saved.fallbackToSystem);
}

export async function setFallbackToSystem(enabled: boolean): Promise<void> {
  if (dnsEditorsLocked.value) return;
  const next = {
    remoteServers: [...dnsState.remoteServers],
    fallbackToSystem: enabled,
  };
  const saved = await saveDnsSettings(next);
  dnsState.remoteServers = saved.remoteServers;
  dnsState.fallbackToSystem = Boolean(saved.fallbackToSystem);
}
