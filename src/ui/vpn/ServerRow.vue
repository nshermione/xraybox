<script setup lang="ts">
import Flag from "../Flag.vue";
import { t } from "../../state/locale";
import type { Server } from "../../state/subscription";

defineProps<{
  server: Server;
  selected?: boolean;
}>();

const emit = defineEmits<{
  click: [];
}>();
</script>

<template>
  <li class="row" :class="{ selected }" @click="emit('click')">
    <Flag :code="server.countryCode" />
    <div class="copy">
      <strong>{{ server.name }}</strong>
      <span>{{ server.region }}</span>
    </div>
    <span class="latency">{{ server.latencyMs == null ? t("home.placeholder") : `${server.latencyMs} ms` }}</span>
    <span v-if="selected" class="check" :aria-label="t('vpn.selected')">✓</span>
  </li>
</template>

<style scoped>
.row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: var(--row-min);
  padding: 10px 16px;
  border-bottom: 1px solid var(--color-border);
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}

.row.selected {
  box-shadow: inset 3px 0 0 var(--color-accent);
  background: color-mix(in srgb, var(--color-accent) 16%, var(--color-surface));
}

.row.selected .copy strong {
  color: var(--color-accent);
}

.row:last-child {
  border-bottom: 0;
}

.row:active {
  background: var(--color-press);
}

.copy {
  flex: 1;
  min-width: 0;
}

.copy strong,
.copy span {
  display: block;
}

.copy strong {
  font-size: 16px;
  font-weight: 500;
}

.copy span {
  color: var(--color-subtle);
  font-size: 12px;
}

.latency {
  color: var(--color-muted);
  font-size: 13px;
  font-weight: 500;
  font-variant-numeric: tabular-nums;
}

.check {
  color: var(--color-accent);
  font-size: 18px;
  font-weight: 700;
}
</style>
