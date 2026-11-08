<script setup lang="ts">
import { computed } from "vue";
import { t } from "../../state/locale";
import type { ConnectionStatus } from "../../state/vpn";

defineProps<{
  status: ConnectionStatus;
}>();

const labels = computed<Record<ConnectionStatus, string>>(() => ({
  disconnected: t("vpn.disconnected"),
  connecting: t("vpn.connectingStatus"),
  connected: t("vpn.connected"),
  disconnecting: t("vpn.disconnectingStatus"),
}));
</script>

<template>
  <p class="badge" :class="status">{{ labels[status] }}</p>
</template>

<style scoped>
.badge {
  margin: 0;
  font-size: 15px;
  /* font-weight: 600; */
  color: var(--color-muted);
}

.badge.connected {
  color: var(--color-success);

}

.badge.connecting {
  color: var(--color-accent);
}

.badge.disconnecting {
  color: #ff9f0a;
}
</style>
