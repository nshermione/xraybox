import { getManualEndpoint, saveManualEndpoint as persistManualEndpoint } from "../engine/tauri";
import { vpnState } from "./vpn";

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

export function applyManualRemark(remark: string | null | undefined): void {
  const trimmed = remark?.trim() ?? "";
  vpnState.manualRemark = trimmed.length > 0 ? trimmed : null;
}

export async function loadManualEndpoint(): Promise<VlessRealityProfile | null> {
  const profile = await getManualEndpoint();
  applyManualRemark(profile?.remark);
  return profile;
}

export async function saveManualEndpoint(profile: VlessRealityProfile): Promise<void> {
  await persistManualEndpoint(profile);
  applyManualRemark(profile.remark);
}

export async function hydrateManualEndpoint(): Promise<void> {
  try {
    await loadManualEndpoint();
  } catch {
    // Vite-only preview has no Tauri runtime.
  }
}
