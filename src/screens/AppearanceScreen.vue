<script setup lang="ts">
import { useNavigation } from "../navigation/useNavigation";
import { t } from "../state/locale";
import { setThemePreference, THEME_OPTION_IDS, themePreference } from "../state/theme";
import Screen from "../ui/Screen.vue";
import List from "../ui/List.vue";
import ListItem from "../ui/ListItem.vue";

const { back } = useNavigation();

function optionColor(id: string): string {
  if (id === "dark") return "hsl(240 8% 28%)";
  if (id === "light") return "hsl(40 80% 56%)";
  return "hsl(250 60% 50%)";
}
</script>

<template>
  <Screen
    :title="t('appearance.title')"
    :parent-title="t('nav.settings')"
    compact
    @back="back"
  >
    <List :title="t('appearance.theme')">
      <ListItem
        v-for="id in THEME_OPTION_IDS"
        :key="id"
        :title="t(`theme.options.${id}.title`)"
        :subtitle="t(`theme.options.${id}.subtitle`)"
        :icon-letter="t(`theme.options.${id}.title`).slice(0, 1)"
        :icon-color="optionColor(id)"
        clickable
        @click="setThemePreference(id)"
      >
        <template #trailing>
          <span v-if="themePreference === id" class="check" :aria-label="t('appearance.selected')">✓</span>
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
