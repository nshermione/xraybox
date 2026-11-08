<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useNavigation } from "../navigation/useNavigation";
import Screen from "../ui/Screen.vue";
import List from "../ui/List.vue";
import ListItem from "../ui/ListItem.vue";
import ConnectButton from "../ui/vpn/ConnectButton.vue";
import SessionStats from "../ui/vpn/SessionStats.vue";
import StatusBadge from "../ui/vpn/StatusBadge.vue";
import Modal from "../ui/Modal.vue";
import TextField from "../ui/TextField.vue";
import Button from "../ui/Button.vue";
import QrImport from "../ui/QrImport.vue";
import { t } from "../state/locale";
import Flag from "../ui/Flag.vue";
import { isDesktop } from "../platform";
import { engineSettings, setSmartConnect } from "../state/engineSettings";
import {
  displayedServer,
  sessionNow,
  sessionState,
  smartConnectActive,
  toggleConnect,
  vpnState,
} from "../state/vpn";
import { communityState } from "../state/community";
import {
  hydrateSubscription,
  refreshPings,
  saveSubscriptionUrl,
  servers,
  subscriptionState,
} from "../state/subscription";

const { push } = useNavigation();
const showAdd = ref(false);
const showEmpty = ref(false);
const qrImport = ref<{ pickImage: () => void; openCamera: () => void } | null>(null);
const showSmartConnectConfirm = ref(false);
const subscriptionUrl = ref("");
const addError = ref<string | null>(null);
const adding = ref(false);

watch(
  () => vpnState.lastError,
  (message) => {
    if (message) {
      console.error("[xraybox] connect error", message);
    }
  },
  { immediate: true },
);

const isConnected = computed(() => vpnState.status === "connected");
const placeholder = computed(() => t("home.placeholder"));

const duration = computed(() => {
  if (!isConnected.value || sessionState.startedAt == null) {
    return placeholder.value;
  }
  return formatDuration(Math.max(0, sessionNow.value - sessionState.startedAt));
});
const usageTotal = computed(() => {
  if (!isConnected.value || sessionState.bytes == null) {
    return placeholder.value;
  }
  return formatBytes(sessionState.bytes);
});
const quotaTotal = computed(() => {
  if (communityState.usedBytes == null) {
    return "";
  }
  const used = formatBytes(communityState.usedBytes);
  if (!communityState.totalBytes) {
    return used;
  }
  return `${used} / ${formatBytes(communityState.totalBytes)}`;
});
const currentIp = computed(() => sessionState.ip || placeholder.value);
const currentCountryCode = computed(() => sessionState.countryCode || undefined);
const ping = computed(() =>
  displayedServer.value.id && displayedServer.value.latencyMs != null
    ? `${displayedServer.value.latencyMs} ms`
    : placeholder.value,
);

const smartConnectReady = computed(
  () => smartConnectActive.value && servers.value.length > 0,
);

async function syncSmartCountry(): Promise<void> {
  if (!smartConnectReady.value) return;
  await hydrateSubscription();
  await refreshPings();
}

watch(smartConnectReady, (ready) => {
  if (ready) void syncSmartCountry();
}, { immediate: true });

function onVpnVisible(): void {
  if (document.visibilityState !== "visible") return;
  void syncSmartCountry();
}

onMounted(() => {
  document.addEventListener("visibilitychange", onVpnVisible);
});

onUnmounted(() => {
  document.removeEventListener("visibilitychange", onVpnVisible);
});

function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  return [hours, minutes, seconds].map((part) => String(part).padStart(2, "0")).join(":");
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 ** 2) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  if (bytes < 1024 ** 3) {
    return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  }
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

function openServers(): void {
  if (smartConnectActive.value) {
    showSmartConnectConfirm.value = true;
    return;
  }
  push({
    name: "servers",
    title: t("servers.title"),
    parentTitle: t("nav.home"),
  });
}

function stayOnSmartConnect(): void {
  showSmartConnectConfirm.value = false;
}

async function leaveSmartConnect(): Promise<void> {
  showSmartConnectConfirm.value = false;
  await setSmartConnect(false);
  if (engineSettings.smartConnect) return;
  push({
    name: "servers",
    title: t("servers.title"),
    parentTitle: t("nav.home"),
  });
}

function onPower(): void {
  if (vpnState.status === "disconnected" && servers.value.length === 0) {
    showEmpty.value = true;
    return;
  }
  void toggleConnect();
}

function openQr(): void {
  showEmpty.value = false;
  if (isDesktop) {
    qrImport.value?.pickImage();
    return;
  }
  qrImport.value?.openCamera();
}

function openAddFromEmpty(): void {
  showEmpty.value = false;
  showAdd.value = true;
}

async function pasteUrl(): Promise<void> {
  try {
    subscriptionUrl.value = (await navigator.clipboard.readText()).trim();
  } catch {
    addError.value = t("subscription.clipboardDenied");
  }
}

async function submitSubscription(url: string): Promise<void> {
  adding.value = true;
  addError.value = null;
  try {
    await saveSubscriptionUrl(url);
    showAdd.value = false;
    subscriptionUrl.value = "";
  } catch (error) {
    addError.value = error instanceof Error ? error.message : t("subscription.invalidUrl");
    showAdd.value = true;
    subscriptionUrl.value = url;
  } finally {
    adding.value = false;
  }
}

function onQrUrl(url: string): void {
  void submitSubscription(url);
}
</script>

<template>
  <Screen>
    <template #leading>
      <button
        type="button"
        class="header-btn"
        :aria-label="t('home.scan')"
        @click="openQr"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="currentColor"
            d="M3 3h8v8H3zm2 2v4h4V5zm8-2h8v8h-8zm2 2v4h4V5zM3 13h8v8H3zm2 2v4h4v-4zm12-2h-2v3h-3v2h3v3h2v-3h3v-2h-3z"
          />
        </svg>
      </button>
    </template>
    <template #trailing>
      <button type="button" class="header-btn" :aria-label="t('home.add')" @click="showAdd = true">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path fill="currentColor" d="M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6z" />
        </svg>
      </button>
    </template>

    <div class="hero">
      <ConnectButton :status="vpnState.status" @click="onPower" />
      <p v-if="vpnState.reloading" class="switching">{{ t("vpn.switching") }}</p>
      <StatusBadge v-else :status="vpnState.status" />
      <p v-if="vpnState.lastError" class="engine-error">{{ vpnState.lastError }}</p>
    </div>

    <List :title="t('home.server')">
      <ListItem
        :title="displayedServer.name"
        :subtitle="displayedServer.region"
        :value="t('home.change')"
        clickable
        @click="openServers"
      >
        <template #leading>
          <Flag :code="displayedServer.countryCode" />
        </template>
        <template #value>
          {{ t("home.change") }}
          <svg class="location-icon" viewBox="0 0 16 16" aria-hidden="true">
            <path
              d="M8 14.5s5-4.2 5-8A5 5 0 1 0 3 6.5c0 3.8 5 8 5 8Z"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linejoin="round"
            />
            <circle
              cx="8"
              cy="6.5"
              r="1.6"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
            />
          </svg>
        </template>
      </ListItem>
    </List>

    <SessionStats
      :duration="duration"
      :usage-total="usageTotal"
      :quota-total="quotaTotal"
      :current-ip="currentIp"
      :current-country-code="currentCountryCode"
      :ping="ping"
      :pinging="subscriptionState.pinging"
      :ping-disabled="servers.length === 0"
      @refresh-ping="refreshPings"
    />

    <Modal
      v-if="showSmartConnectConfirm"
      :title="t('vpn.smartConnect')"
      :closable="false"
      @close="stayOnSmartConnect"
    >
      <p class="modal-copy">{{ t("vpn.smartConnectStayMessage") }}</p>
      <div class="modal-actions">
        <Button @click="stayOnSmartConnect">{{ t("home.yes") }}</Button>
        <Button variant="secondary" @click="leaveSmartConnect">{{ t("home.no") }}</Button>
      </div>
    </Modal>

    <Modal v-if="showEmpty" :title="t('home.noServerTitle')" @close="showEmpty = false">
      <p class="modal-copy">{{ t("home.noServerMessage") }}</p>
      <div class="modal-actions">
        <Button @click="openQr">{{ isDesktop ? t("home.qrImage") : t("home.scan") }}</Button>
        <Button variant="secondary" @click="openAddFromEmpty">{{ t("home.addLink") }}</Button>
      </div>
    </Modal>

    <Modal v-if="showAdd" :title="t('home.addSubscription')" @close="showAdd = false">
      <TextField
        v-model="subscriptionUrl"
        :label="t('home.subscriptionUrl')"
        :placeholder="t('home.subscriptionUrlPlaceholder')"
        :disabled="adding"
      >
        <template #trailing>
          <button type="button" class="paste" @click="pasteUrl">{{ t("home.paste") }}</button>
        </template>
      </TextField>
      <p v-if="addError" class="engine-error">{{ addError }}</p>
      <Button :disabled="adding || !subscriptionUrl.trim()" @click="submitSubscription(subscriptionUrl)">
        {{ t("nested.save") }}
      </Button>
    </Modal>
    <QrImport ref="qrImport" @url="onQrUrl" />
  </Screen>
</template>

<style scoped>
.hero {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  padding: 28px 0 32px;
  overflow: visible;
}

.switching {
  margin: 0;
  color: var(--color-accent);
  font-size: 15px;
}

.engine-error,
.modal-copy {
  max-width: 28rem;
  margin: 0;
  padding: 0 16px;
  font-size: 13px;
  line-height: 1.4;
  text-align: center;
}

.engine-error {
  color: var(--color-danger, #c0392b);
}

.modal-copy {
  padding: 0 0 16px;
  color: var(--color-muted);
  text-align: left;
}

.modal-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.location-icon {
  width: 15px;
  height: 15px;
}

.paste {
  min-height: 32px;
  padding: 0 10px;
  border: 0;
  border-radius: 8px;
  background: var(--color-press);
  color: var(--color-accent);
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
}
</style>
