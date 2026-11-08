<script setup lang="ts">
import { computed } from "vue";
import type { PackState, RuleAction, ServiceOverride } from "../engine/tauri";
import { useNavigation } from "../navigation/useNavigation";
import { packById, packsLocked, updatePack } from "../state/packs";
import { t } from "../state/locale";
import Screen from "../ui/Screen.vue";
import List from "../ui/List.vue";
import ListItem from "../ui/ListItem.vue";
import SegmentedControl from "../ui/SegmentedControl.vue";
import Button from "../ui/Button.vue";

const props = defineProps<{
  packId: string;
}>();

const { back } = useNavigation();

const SERVICES: Record<string, string[]> = {
  google: ["youtube", "google-play"],
  apple: ["icloud"],
  meta: ["facebook", "instagram", "whatsapp", "messenger"],
  tiktok: ["tiktok", "bytedance"],
  microsoft: ["onedrive"],
  china: ["google-cn", "apple-cn"],
};

const pack = computed(() => packById(props.packId));
const serviceIds = computed(() => SERVICES[props.packId] ?? []);
const actionOptions = computed(() =>
  (["direct", "proxy", "block"] as RuleAction[]).map((id) => ({
    id,
    label: t(`routing.actions.${id}`),
  })),
);

function serviceAction(packValue: PackState, serviceId: string): RuleAction {
  return packValue.services.find((service) => service.id === serviceId)?.action ?? packValue.action;
}

function setServiceAction(serviceId: string, action: string): void {
  const current = pack.value;
  if (!current || packsLocked.value) return;
  const services: ServiceOverride[] = serviceIds.value.map((id) => ({
    id,
    action: id === serviceId ? (action as RuleAction) : serviceAction(current, id),
  }));
  void updatePack({ ...current, services });
}

function resetOverrides(): void {
  const current = pack.value;
  if (!current || packsLocked.value) return;
  void updatePack({ ...current, services: [] });
}
</script>

<template>
  <Screen
    :title="t('routing.advancedTitle', { name: t(`routing.packs.${packId}.title`) })"
    :parent-title="t('routing.title')"
    compact
    @back="back"
  >
    <p v-if="!pack" class="hint">{{ t("routing.missingPack") }}</p>
    <template v-else>
      <p class="hint">{{ t("routing.advancedHint") }}</p>
      <List :title="t('routing.services')">
        <ListItem
          v-for="serviceId in serviceIds"
          :key="serviceId"
          :title="t(`routing.servicesCatalog.${serviceId}`)"
          :subtitle="t(`routing.actions.${serviceAction(pack, serviceId)}`)"
          :icon-letter="t(`routing.servicesCatalog.${serviceId}`).slice(0, 1)"
          icon-color="hsl(210 70% 48%)"
        >
          <template #trailing>
            <div class="actions">
              <SegmentedControl
                :model-value="serviceAction(pack, serviceId)"
                :options="actionOptions"
                @update:model-value="setServiceAction(serviceId, $event)"
              />
            </div>
          </template>
        </ListItem>
      </List>
      <Button :disabled="packsLocked" @click="resetOverrides">{{ t("routing.reset") }}</Button>
    </template>
  </Screen>
</template>

<style scoped>
.hint {
  margin: 0 4px 12px;
  color: var(--color-muted);
  font-size: 13px;
  line-height: 1.4;
}

.actions {
  min-width: 180px;
}
</style>
