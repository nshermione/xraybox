import { computed, reactive } from "vue";
import { getRoutePacks, saveRoutePacks, type PackState, type RuleAction } from "../engine/tauri";
import { vpnState } from "./vpn";

export const PACK_IDS = [
  "google",
  "apple",
  "meta",
  "tiktok",
  "x",
  "microsoft",
  "amazon",
  "netflix",
  "telegram",
  "discord",
  "openai",
  "github",
  "china",
] as const;

export const packState = reactive({
  packs: [] as PackState[],
});

export const packsLocked = computed(() => vpnState.status !== "disconnected");

export const enabledPackCount = computed(
  () => packState.packs.filter((pack) => pack.enabled).length,
);

export async function hydrateRoutePacks(): Promise<void> {
  try {
    packState.packs = await getRoutePacks();
  } catch {
    packState.packs = defaultPacks();
  }
}

export function packById(id: string): PackState | undefined {
  return packState.packs.find((pack) => pack.id === id);
}

export async function updatePack(next: PackState): Promise<void> {
  if (packsLocked.value) return;
  const packs = packState.packs.map((pack) => (pack.id === next.id ? next : pack));
  packState.packs = await saveRoutePacks(packs);
}

function defaultPacks(): PackState[] {
  return PACK_IDS.map((id) => ({
    id,
    enabled: id === "china",
    action: id === "china" ? "direct" : "proxy",
    services: [],
  }));
}

export function actionLabelKey(action: RuleAction): string {
  return `routing.actions.${action}`;
}
