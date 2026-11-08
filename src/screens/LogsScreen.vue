<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { getAppLog, getEngineLog } from "../engine/tauri";
import { useNavigation } from "../navigation/useNavigation";
import { engineSettings, setLogsEnabled } from "../state/engineSettings";
import { t } from "../state/locale";
import EmptyState from "../ui/EmptyState.vue";
import List from "../ui/List.vue";
import ListItem from "../ui/ListItem.vue";
import Screen from "../ui/Screen.vue";
import SegmentedControl from "../ui/SegmentedControl.vue";
import Switch from "../ui/Switch.vue";

const LINE_CAP = 400;
const FOLLOW_THRESHOLD_PX = 48;
const POLL_MS = 500;

const { back } = useNavigation();
const tab = ref("xray");
const xrayLog = ref("");
const appLog = ref("");
const pane = ref<HTMLElement | null>(null);
const following = ref(true);
let timer = 0;

const tabs = computed(() => [
  { id: "xray", label: t("logs.tabXray") },
  { id: "app", label: t("logs.tabApp") },
]);

const activeLog = computed(() => (tab.value === "xray" ? xrayLog.value : appLog.value));
const emptyMessage = computed(() =>
  tab.value === "xray" ? t("logs.emptyXray") : t("logs.emptyApp"),
);

function keepLastLines(text: string): string {
  const lines = text.replace(/\r\n/g, "\n").replace(/\n+$/, "").split("\n");
  if (lines.length === 1 && lines[0] === "") return "";
  if (lines.length <= LINE_CAP) return lines.join("\n");
  return lines.slice(-LINE_CAP).join("\n");
}

function isNearBottom(el: HTMLElement): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= FOLLOW_THRESHOLD_PX;
}

function scrollToBottom(): void {
  const el = pane.value;
  if (!el) return;
  el.scrollTop = el.scrollHeight;
}

function onPaneScroll(): void {
  const el = pane.value;
  if (!el) return;
  following.value = isNearBottom(el);
}

async function applyLog(target: typeof xrayLog, next: string): Promise<void> {
  const trimmed = keepLastLines(next);
  if (target.value === trimmed) return;
  const stick = following.value;
  target.value = trimmed;
  if (!stick) return;
  await nextTick();
  scrollToBottom();
}

async function refresh(): Promise<void> {
  if (!engineSettings.logsEnabled) return;
  try {
    if (tab.value === "xray") {
      await applyLog(xrayLog, await getEngineLog());
    } else {
      await applyLog(appLog, await getAppLog());
    }
  } catch {
    // Vite-only preview has no Tauri runtime.
  }
}

function stopPoll(): void {
  if (!timer) return;
  window.clearInterval(timer);
  timer = 0;
}

function startPoll(): void {
  stopPoll();
  if (!engineSettings.logsEnabled) return;
  following.value = true;
  void refresh();
  timer = window.setInterval(() => {
    void refresh();
  }, POLL_MS);
}

onMounted(startPoll);
onUnmounted(stopPoll);

watch(() => engineSettings.logsEnabled, startPoll);
watch(tab, () => {
  following.value = true;
  if (engineSettings.logsEnabled) {
    void refresh().then(() => {
      if (following.value) scrollToBottom();
    });
  }
});
</script>

<template>
  <Screen
    :title="t('settings.items.logs.title')"
    :parent-title="t('nav.settings')"
    compact
    fill
    @back="back"
  >
    <div class="logs-page">
      <List>
        <ListItem :title="t('logs.enabled')" icon-letter="L" icon-color="hsl(145 60% 40%)">
          <template #trailing>
            <Switch :model-value="engineSettings.logsEnabled" @update:model-value="setLogsEnabled" />
          </template>
        </ListItem>
      </List>

      <template v-if="engineSettings.logsEnabled">
        <div class="tabs">
          <SegmentedControl :options="tabs" v-model="tab" />
        </div>
        <div ref="pane" class="log-pane" @scroll.passive="onPaneScroll">
          <pre v-if="activeLog" class="log">{{ activeLog }}</pre>
          <EmptyState v-else :title="t('logs.emptyTitle')" :message="emptyMessage" />
        </div>
      </template>
      <p v-else class="hint">{{ t("logs.disabledHint") }}</p>
    </div>
  </Screen>
</template>

<style scoped>
.logs-page {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}

.logs-page :deep(.list-group) {
  flex-shrink: 0;
  margin-bottom: 12px;
}

.tabs {
  flex-shrink: 0;
}

.log-pane {
  flex: 1;
  min-height: 0;
  margin-top: 12px;
  margin-bottom: 8px;
  overflow: auto;
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
  background: var(--color-surface);
  border-radius: var(--radius-md);
}

.log {
  margin: 0;
  padding: 12px 14px;
  color: var(--color-text);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.45;
  white-space: pre-wrap;
  word-break: break-word;
}

.hint {
  margin: 8px 4px 0;
  color: var(--color-muted);
  font-size: 13px;
  line-height: 1.4;
}
</style>
