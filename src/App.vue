<script setup lang="ts">
import type { Component } from "vue";
import { onMounted, onUnmounted, watch } from "vue";
import { markAppPainted } from "./boot/splash";
import { TABS, useNavigation, type TabId } from "./navigation/useNavigation";
import AppShell from "./ui/AppShell.vue";
import ScreenStack from "./ui/ScreenStack.vue";
import HomeScreen from "./screens/HomeScreen.vue";
import ServersScreen from "./screens/ServersScreen.vue";
import SettingsScreen from "./screens/SettingsScreen.vue";
import NestedSettingScreen from "./screens/NestedSettingScreen.vue";
import AppearanceScreen from "./screens/AppearanceScreen.vue";
import LanguageScreen from "./screens/LanguageScreen.vue";
import RoutingScreen from "./screens/RoutingScreen.vue";
import RouteAdvancedScreen from "./screens/RouteAdvancedScreen.vue";
import FetchIntervalScreen from "./screens/FetchIntervalScreen.vue";
import LogsScreen from "./screens/LogsScreen.vue";
import { listenAppShown, listenCommunityEvent } from "./engine/tauri";
import { t } from "./state/locale";
import { fetchOnResume, refreshSubscription } from "./state/subscription";

function onVisible(): void {
  if (document.visibilityState === "visible") {
    void fetchOnResume();
  }
}

let unlistenAppShown: (() => void) | undefined;
let unlistenCommunity: (() => void) | undefined;

onMounted(() => {
  markAppPainted();
  document.addEventListener("visibilitychange", onVisible);
  window.addEventListener("focus", onVisible);
  void listenAppShown(() => {
    void fetchOnResume();
  })
    .then((unlisten) => {
      unlistenAppShown = unlisten;
    })
    .catch(() => {
      // Vite-only preview has no Tauri runtime.
    });
  void listenCommunityEvent((type) => {
    if (type === "sub.update") {
      void refreshSubscription();
    }
  })
    .then((unlisten) => {
      unlistenCommunity = unlisten;
    })
    .catch(() => {
      // Vite-only preview has no Tauri runtime.
    });
});

onUnmounted(() => {
  document.removeEventListener("visibilitychange", onVisible);
  window.removeEventListener("focus", onVisible);
  unlistenAppShown?.();
  unlistenAppShown = undefined;
  unlistenCommunity?.();
  unlistenCommunity = undefined;
});

const { tab, switchTab } = useNavigation();

watch(tab, () => {
  void fetchOnResume();
});

const registry: Record<string, Component> = {
  home: HomeScreen,
  servers: ServersScreen,
  settings: SettingsScreen,
  "nested-setting": NestedSettingScreen,
  appearance: AppearanceScreen,
  language: LanguageScreen,
  routing: RoutingScreen,
  "route-advanced": RouteAdvancedScreen,
  "fetch-interval": FetchIntervalScreen,
  logs: LogsScreen,
};

const icons: Record<TabId, string> = {
  home: "M12 2.2 4 5.5v6.1c0 5.3 3.4 10.2 8 11.4 4.6-1.2 8-6.1 8-11.4V5.5z",
  settings:
    "M10.2 2.5h3.6l.3 2.2a6.8 6.8 0 0 1 1.7.7l2-1.1 2.5 2.5-1.1 2a6.8 6.8 0 0 1 .7 1.7l2.2.3v3.6l-2.2.3a6.8 6.8 0 0 1-.7 1.7l1.1 2-2.5 2.5-2-1.1a6.8 6.8 0 0 1-1.7.7l-.3 2.2h-3.6l-.3-2.2a6.8 6.8 0 0 1-1.7-.7l-2 1.1-2.5-2.5 1.1-2a6.8 6.8 0 0 1-.7-1.7L2.5 13.8v-3.6l2.2-.3a6.8 6.8 0 0 1 .7-1.7l-1.1-2 2.5-2.5 2 1.1a6.8 6.8 0 0 1 1.7-.7zm1.8 6A3.5 3.5 0 1 0 15.5 12 3.5 3.5 0 0 0 12 8.5z",
};
</script>

<template>
  <AppShell>
    <template #nav>
      <button
        v-for="item in TABS"
        :key="item.id"
        type="button"
        class="nav-item"
        :class="{ active: tab === item.id }"
        @click="switchTab(item.id)"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path fill="currentColor" fill-rule="evenodd" :d="icons[item.id]" />
        </svg>
        <span>{{ t(`nav.${item.id}`) }}</span>
      </button>
    </template>

    <ScreenStack :registry="registry" />
  </AppShell>
</template>
