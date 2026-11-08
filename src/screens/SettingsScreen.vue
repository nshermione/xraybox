<script setup lang="ts">
import { computed } from "vue";
import { useNavigation } from "../navigation/useNavigation";
import Screen from "../ui/Screen.vue";
import List from "../ui/List.vue";
import ListItem from "../ui/ListItem.vue";
import Switch from "../ui/Switch.vue";
import { isAndroid, isDesktop, isIos } from "../platform";
import { dnsEditorsLocked, remoteDnsSummary } from "../state/dns";
import { engineSettings, setAutoConnect, setKillSwitch, setSmartConnect, setStartOnBoot } from "../state/engineSettings";
import { subscriptionState } from "../state/subscription";
import { enabledPackCount, packsLocked } from "../state/packs";
import { localeLabel, t } from "../state/locale";
import { themeLabel } from "../state/theme";
import { settingGroups, type SettingItem } from "./settingsData";

const visibleGroups = computed(() =>
  settingGroups
    .map((group) => ({
      ...group,
      items: group.items.filter((item) => {
        if (item.id === "kill-switch" || item.id === "start-boot") {
          return isDesktop || isAndroid || isIos;
        }
        if (item.id === "manual-endpoint") {
          return import.meta.env.DEV;
        }
        return true;
      }),
    }))
    .filter((group) => group.items.length > 0),
);

const { push } = useNavigation();

function itemTitle(item: SettingItem): string {
  return t(`settings.items.${item.id}.title`);
}

function itemSubtitle(item: SettingItem): string | undefined {
  if (item.id === "kill-switch" || item.id === "smart-connect" || item.id === "fetch-interval") {
    return undefined;
  }
  const key =
    item.id === "start-boot" && isIos
      ? "settings.items.start-boot.subtitleIos"
      : item.id === "start-boot" && isAndroid
        ? "settings.items.start-boot.subtitleAndroid"
        : `settings.items.${item.id}.subtitle`;
  const value = t(key);
  return value === key ? undefined : value;
}

function itemHelp(item: SettingItem): string | undefined {
  const helpKey = `settings.items.${item.id}.help`;
  const help = t(helpKey);
  return help === helpKey ? undefined : help;
}

function itemValue(item: SettingItem): string | undefined {
  if (item.id === "appearance") return themeLabel.value;
  if (item.id === "language") return localeLabel.value;
  if (item.id === "remote-dns") return remoteDnsSummary.value;
  if (item.id === "rules") return String(1 + enabledPackCount.value);
  if (item.id === "fetch-interval") return t(`fetchInterval.options.${subscriptionState.fetchInterval}.title`);
  return item.kind === "value" ? item.value : undefined;
}

function isLocked(item: SettingItem): boolean {
  return (item.id === "remote-dns" && dnsEditorsLocked.value) || (item.id === "rules" && packsLocked.value);
}

function toggleValue(item: SettingItem): boolean {
  if (item.id === "auto-connect") return engineSettings.autoConnect;
  if (item.id === "start-boot") return engineSettings.startOnBoot;
  if (item.id === "kill-switch") return engineSettings.killSwitch;
  if (item.id === "smart-connect") return engineSettings.smartConnect;
  return false;
}

function onToggle(item: SettingItem, enabled: boolean): void {
  if (isLocked(item)) return;
  if (item.id === "auto-connect") {
    void setAutoConnect(enabled);
    return;
  }
  if (item.id === "start-boot") {
    void setStartOnBoot(enabled);
    return;
  }
  if (item.id === "kill-switch") {
    void setKillSwitch(enabled);
    return;
  }
  if (item.id === "smart-connect") {
    void setSmartConnect(enabled);
    return;
  }
}

function openItem(item: SettingItem): void {
  if (item.kind === "toggle" || isLocked(item)) return;

  if (item.id === "appearance") {
    push({
      name: "appearance",
      title: t("appearance.title"),
      parentTitle: t("nav.settings"),
    });
    return;
  }

  if (item.id === "rules") {
    push({
      name: "routing",
      title: t("routing.title"),
      parentTitle: t("nav.settings"),
    });
    return;
  }

  if (item.id === "language") {
    push({
      name: "language",
      title: t("language.title"),
      parentTitle: t("nav.settings"),
    });
    return;
  }

  if (item.id === "fetch-interval") {
    push({
      name: "fetch-interval",
      title: t("fetchInterval.title"),
      parentTitle: t("nav.settings"),
    });
    return;
  }

  if (item.id === "logs") {
    push({
      name: "logs",
      title: t("settings.items.logs.title"),
      parentTitle: t("nav.settings"),
    });
    return;
  }

  push({
    name: "nested-setting",
    title: itemTitle(item),
    parentTitle: t("nav.settings"),
    props: {
      titleKey: `settings.items.${item.id}.title`,
      parentTitleKey: "nav.settings",
      screen: item.screen ?? "list",
    },
  });
}
</script>

<template>
  <Screen :title="t('settings.title')">
    <List v-for="group in visibleGroups" :key="group.id" :title="t(`settings.groups.${group.id}`)">
      <ListItem
        v-for="item in group.items"
        :key="item.id"
        :title="itemTitle(item)"
        :subtitle="itemSubtitle(item)"
        :help="itemHelp(item)"
        :help-label="t('nested.moreInfo')"
        :icon-letter="itemTitle(item).slice(0, 1)"
        :icon-color="item.iconColor"
        :value="itemValue(item)"
        :chevron="item.kind !== 'toggle'"
        :clickable="item.kind !== 'toggle' && !isLocked(item)"
        @click="item.kind !== 'toggle' ? openItem(item) : undefined"
      >
        <template v-if="item.kind === 'toggle'" #trailing>
          <Switch :model-value="toggleValue(item)" :disabled="isLocked(item)" @update:model-value="onToggle(item, $event)" />
        </template>
      </ListItem>
    </List>
  </Screen>
</template>
