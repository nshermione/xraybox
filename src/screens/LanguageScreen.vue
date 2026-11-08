<script setup lang="ts">
import { useNavigation } from "../navigation/useNavigation";
import { LOCALE_OPTION_IDS, localePreference, setLocalePreference, t } from "../state/locale";
import Screen from "../ui/Screen.vue";
import List from "../ui/List.vue";
import ListItem from "../ui/ListItem.vue";

const { back } = useNavigation();

function optionColor(id: string): string {
  if (id === "vi") return "hsl(145 55% 40%)";
  if (id === "en") return "hsl(210 70% 48%)";
  return "hsl(200 45% 46%)";
}
</script>

<template>
  <Screen
    :title="t('language.title')"
    :parent-title="t('nav.settings')"
    compact
    @back="back"
  >
    <List :title="t('language.listTitle')">
      <ListItem
        v-for="id in LOCALE_OPTION_IDS"
        :key="id"
        :title="t(`language.options.${id}.title`)"
        :subtitle="t(`language.options.${id}.subtitle`)"
        :icon-letter="t(`language.options.${id}.title`).slice(0, 1)"
        :icon-color="optionColor(id)"
        clickable
        @click="setLocalePreference(id)"
      >
        <template #trailing>
          <span v-if="localePreference === id" class="check" :aria-label="t('language.selected')">✓</span>
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
