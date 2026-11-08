<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import Screen from "../ui/Screen.vue";
import List from "../ui/List.vue";
import EmptyState from "../ui/EmptyState.vue";
import ServerRow from "../ui/vpn/ServerRow.vue";
import SegmentedControl from "../ui/SegmentedControl.vue";
import Modal from "../ui/Modal.vue";
import Button from "../ui/Button.vue";
import { useNavigation } from "../navigation/useNavigation";
import { countryName } from "../geo/country";
import { t } from "../state/locale";
import { reconnect, selectServer, smartConnectActive, vpnState } from "../state/vpn";
import { isDesktop } from "../platform";
import {
  clearAllProfiles,
  refreshPings,
  refreshSubscription,
  servers,
  subscriptionState,
} from "../state/subscription";

const { back } = useNavigation();
const view = ref("country");
const showClear = ref(false);
const clearing = ref(false);

const modes = computed(() => [
  { id: "country", label: t("servers.country") },
  { id: "profiles", label: t("servers.profiles") },
]);

const countryGroups = computed(() => {
  const groups = new Map<string, typeof servers.value>();
  for (const server of servers.value) {
    const code = server.countryCode || "xx";
    const list = groups.get(code) ?? [];
    list.push(server);
    groups.set(code, list);
  }
  return [...groups.entries()]
    .map(([code, items]) => ({
      code,
      title: countryName(code) || t("servers.other"),
      items: items.slice().sort((a, b) => (a.latencyMs ?? 99999) - (b.latencyMs ?? 99999)),
    }))
    .sort((a, b) => a.title.localeCompare(b.title));
});

const profileList = computed(() =>
  servers.value.slice().sort((a, b) => a.name.localeCompare(b.name)),
);

async function chooseServer(id: string): Promise<void> {
  if (smartConnectActive.value) return;
  const changed = id !== vpnState.selectedServerId;
  await selectServer(id);
  back();
  if (changed) {
    void reconnect();
  }
}

async function confirmClear(): Promise<void> {
  clearing.value = true;
  try {
    await clearAllProfiles();
    showClear.value = false;
  } finally {
    clearing.value = false;
  }
}

onMounted(() => {
  if (servers.value.length > 0) {
    void refreshPings();
  }
});
</script>

<template>
  <Screen
    :title="t('servers.title')"
    :parent-title="t('nav.home')"
    compact
    :pull-refresh="!isDesktop"
    @back="back"
    @refresh="refreshPings"
  >
    <template #trailing>
      <button
        type="button"
        class="header-btn header-text"
        :disabled="subscriptionState.fetching || !subscriptionState.url"
        :aria-label="t('servers.updateSubscription')"
        @click="refreshSubscription"
      >
        {{ t("servers.update") }}
      </button>
    </template>
    <div class="toolbar">
      <SegmentedControl :options="modes" v-model="view" />
      <p v-if="subscriptionState.fetching || subscriptionState.pinging" class="status">
        {{ subscriptionState.fetching ? t("servers.updating") : t("servers.pinging") }}
      </p>
      <p v-else-if="subscriptionState.lastError" class="error">{{ subscriptionState.lastError }}</p>
    </div>

    <EmptyState
      v-if="servers.length === 0"
      :title="t('servers.emptyTitle')"
      :message="t('servers.emptyMessage')"
    />
    <template v-else-if="view === 'country'">
      <List v-for="group in countryGroups" :key="group.code" :title="group.title">
        <ServerRow
          v-for="server in group.items"
          :key="`country-${server.id}`"
          :server="server"
          :selected="server.id === vpnState.selectedServerId"
          @click="chooseServer(server.id)"
        />
      </List>
    </template>
    <List v-else :title="t('servers.profiles')">
      <ServerRow
        v-for="server in profileList"
        :key="`profile-${server.id}`"
        :server="server"
        :selected="server.id === vpnState.selectedServerId"
        @click="chooseServer(server.id)"
      />
    </List>

    <div v-if="servers.length > 0" class="clear-wrap">
      <Button
        variant="destructive"
        :aria-label="t('servers.clearProfiles')"
        @click="showClear = true"
      >
        {{ t("servers.clearProfiles") }}
      </Button>
    </div>

    <Modal v-if="showClear" :title="t('servers.clearConfirmTitle')" @close="showClear = false">
      <p class="modal-copy">{{ t("servers.clearConfirmMessage") }}</p>
      <div class="modal-actions">
        <Button variant="destructive" :disabled="clearing" @click="confirmClear">
          {{ t("servers.clearConfirm") }}
        </Button>
        <Button variant="secondary" @click="showClear = false">{{ t("home.cancel") }}</Button>
      </div>
    </Modal>
  </Screen>
</template>

<style scoped>
.toolbar {
  margin-bottom: 16px;
}

.clear-wrap {
  margin-top: 28px;
  padding-bottom: 40px;
}

.clear-wrap :deep(.btn) {
  font-weight: 500;
}

.status,
.error,
.modal-copy {
  margin: 10px 4px 0;
  font-size: 13px;
}

.status {
  color: var(--color-muted);
}

.error {
  color: var(--color-danger);
}

.modal-copy {
  margin: 0 0 16px;
  color: var(--color-muted);
}

.modal-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
</style>
