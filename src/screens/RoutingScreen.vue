<script setup lang="ts">
import { computed } from "vue";
import { type PackState, type RuleAction } from "../engine/tauri";
import { useNavigation } from "../navigation/useNavigation";
import { packById, packsLocked, packState, updatePack } from "../state/packs";
import { t } from "../state/locale";
import Screen from "../ui/Screen.vue";
import List from "../ui/List.vue";
import ListItem from "../ui/ListItem.vue";
import Switch from "../ui/Switch.vue";

const { back, push } = useNavigation();

const packs = computed(() => packState.packs);

const ACTIONS: RuleAction[] = ["direct", "proxy", "block"];

function packTitle(id: string): string {
  return t(`routing.packs.${id}.title`);
}

function packSubtitle(pack: PackState): string {
  return t(`routing.actions.${pack.action}`);
}

const ADVANCED_PACK_IDS = new Set([
  "google",
  "apple",
  "meta",
  "tiktok",
  "microsoft",
  "china",
]);

function hasAdvanced(id: string): boolean {
  return ADVANCED_PACK_IDS.has(id);
}

function cycleAction(pack: PackState): void {
  if (packsLocked.value) return;
  const index = ACTIONS.indexOf(pack.action);
  const next = ACTIONS[(index + 1) % ACTIONS.length];
  void updatePack({ ...pack, action: next });
}

function togglePack(pack: PackState, enabled: boolean): void {
  void updatePack({ ...pack, enabled });
}

function openAdvanced(pack: PackState): void {
  if (packsLocked.value || !hasAdvanced(pack.id)) return;
  push({
    name: "route-advanced",
    title: t("routing.advancedTitle", { name: packTitle(pack.id) }),
    parentTitle: t("routing.title"),
    props: { packId: pack.id },
  });
}

</script>

<template>
  <Screen :title="t('routing.title')" :parent-title="t('nav.settings')" compact @back="back">
    <p class="hint">{{ t("routing.hint") }}</p>
    <List :title="t('routing.listTitle')">
      <ListItem
        v-for="pack in packs"
        :key="pack.id"
        :title="packTitle(pack.id)"
        :subtitle="packSubtitle(pack)"
        :icon-letter="packTitle(pack.id).slice(0, 1)"
        icon-color="hsl(20 75% 48%)"
        :clickable="!packsLocked"
        @click="cycleAction(pack)"
      >
        <template #trailing>
          <Switch
            :model-value="pack.enabled"
            :disabled="packsLocked"
            @update:model-value="togglePack(pack, $event)"
          />
        </template>
      </ListItem>
    </List>
    <button
      v-for="pack in packs.filter((item) => hasAdvanced(item.id))"
      :key="`${pack.id}-adv`"
      type="button"
      class="advanced"
      :disabled="packsLocked || !packById(pack.id)?.enabled"
      @click="openAdvanced(pack)"
    >
      {{ t("routing.advancedLink", { name: packTitle(pack.id) }) }}
    </button>
  </Screen>
</template>

<style scoped>
.hint {
  margin: 0 4px 12px;
  color: var(--color-muted);
  font-size: 13px;
  line-height: 1.4;
}

.advanced {
  display: block;
  width: 100%;
  margin: 8px 0 0;
  padding: 10px 4px;
  border: 0;
  background: transparent;
  color: var(--color-accent);
  font-size: 14px;
  text-align: left;
  cursor: pointer;
}

.advanced:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
