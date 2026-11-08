import { reactive } from "vue";
import { getCommunityUsage, type CommunityUsage } from "../engine/tauri";

export const communityState = reactive({
  usedBytes: null as number | null,
  totalBytes: null as number | null,
  expiryAt: null as string | null,
  lastError: null as string | null,
});

export function applyCommunityUsage(usage: CommunityUsage): void {
  communityState.usedBytes = usage.usedBytes;
  communityState.totalBytes = usage.totalBytes;
  communityState.expiryAt = usage.expiryAt;
  communityState.lastError = null;
}

export function clearCommunityUsage(): void {
  communityState.usedBytes = null;
  communityState.totalBytes = null;
  communityState.expiryAt = null;
  communityState.lastError = null;
}

export async function refreshCommunityUsage(): Promise<void> {
  try {
    const usage = await getCommunityUsage();
    if (!usage) {
      clearCommunityUsage();
      return;
    }
    applyCommunityUsage(usage);
  } catch {
    clearCommunityUsage();
  }
}
