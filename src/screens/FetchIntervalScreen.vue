<script setup lang="ts">
import { useNavigation } from "../navigation/useNavigation";
import { t } from "../state/locale";
import { FETCH_INTERVAL_IDS, persistFetchInterval, subscriptionState } from "../state/subscription";
import Screen from "../ui/Screen.vue";
import List from "../ui/List.vue";
import ListItem from "../ui/ListItem.vue";

const { back } = useNavigation();

function optionColor(id: string): string {
  if (id === "off") return "hsl(0 0% 40%)";
  if (id === "3h") return "hsl(200 70% 44%)";
  if (id === "7d") return "hsl(260 50% 50%)";
  return "hsl(145 55% 40%)";
}
</script>

<template>
  <Screen
    :title="t('fetchInterval.title')"
    :parent-title="t('nav.settings')"
    compact
    @back="back"
  >
    <List :title="t('fetchInterval.listTitle')">
      <ListItem
        v-for="id in FETCH_INTERVAL_IDS"
        :key="id"
        :title="t(`fetchInterval.options.${id}.title`)"
        :subtitle="t(`fetchInterval.options.${id}.subtitle`)"
        :icon-letter="t(`fetchInterval.options.${id}.title`).slice(0, 1)"
        :icon-color="optionColor(id)"
        clickable
        @click="persistFetchInterval(id)"
      >
        <template #trailing>
          <span
            v-if="subscriptionState.fetchInterval === id"
            class="check"
            :aria-label="t('fetchInterval.selected')"
          >✓</span>
        </template>
      </ListItem>
    </List>
  </Screen>
</template>

<style scoped>
.check {
  color: var(--color-accent);
  font-size: 18px;
  font-weight: 700;
}
</style>
