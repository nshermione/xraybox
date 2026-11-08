<script setup lang="ts">
import { computed } from "vue";
import Flag from "../Flag.vue";
import { countryName } from "../../geo/country";
import { t } from "../../state/locale";

const props = defineProps<{
  duration: string;
  usageTotal: string;
  quotaTotal?: string;
  currentIp: string;
  currentCountryCode?: string;
  ping: string;
  pinging?: boolean;
  pingDisabled?: boolean;
}>();

const emit = defineEmits<{
  refreshPing: [];
}>();

const currentCountry = computed(() =>
  props.currentCountryCode ? countryName(props.currentCountryCode) : "",
);
</script>

<template>
  <section class="session">
    <h2>{{ t("home.session") }}</h2>
    <dl class="card">
      <div class="cell">
        <dt>{{ t("home.duration") }}</dt>
        <dd :class="{ empty: duration === t('home.placeholder') }">{{ duration }}</dd>
      </div>
      <div class="cell">
        <dt>{{ t("home.usage") }}</dt>
        <dd :class="{ empty: usageTotal === t('home.placeholder') }">{{ usageTotal }}</dd>
      </div>
      <div v-if="quotaTotal" class="cell">
        <dt>{{ t("home.quota") }}</dt>
        <dd>{{ quotaTotal }}</dd>
      </div>
      <div class="cell ip">
        <dt>{{ t("home.currentIp") }}</dt>
        <dd :class="{ empty: currentIp === t('home.placeholder') }">{{ currentIp }}</dd>
        <span v-if="currentCountryCode && currentCountry" class="place">
          {{ currentCountry }}
          <Flag :code="currentCountryCode" size="xs" />
        </span>
      </div>
      <div class="cell">
        <dt class="ping-label">
          {{ t("home.ping") }}
          <button
            type="button"
            class="ping-refresh"
            :disabled="pingDisabled || pinging"
            :aria-label="t('servers.refreshPing')"
            @click="emit('refreshPing')"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path
                fill="currentColor"
                d="M4 12a8 8 0 0 1 13.3-6H14v2h6V2h-2v2.6A10 10 0 1 0 22 12h-2a8 8 0 0 1-16 0"
              />
            </svg>
          </button>
        </dt>
        <dd :class="{ empty: ping === t('home.placeholder') }">{{ ping }}</dd>
      </div>
    </dl>
  </section>
</template>

<style scoped>
.session {
  margin-bottom: 22px;
}

.session h2 {
  margin: 0 0 8px 12px;
  color: var(--color-muted);
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.card {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 22px 24px;
  margin: 0;
  padding: 18px 18px 16px;
  background: var(--color-surface);
  border-radius: var(--radius-md);
}

.cell {
  min-width: 0;
}

.cell dt {
  margin: 0 0 6px;
  color: var(--color-muted);
  font-size: 12px;
  font-weight: 500;
}

.cell dd {
  margin: 0;
  color: var(--color-text);
  font-size: 16px;
  font-weight: 500;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  overflow-wrap: anywhere;
}

.cell.ip dd {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 5px 8px;
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0;
}

.cell dd.empty {
  color: var(--color-subtle);
  font-weight: 500;
}

.ping-label {
  display: flex;
  align-items: center;
  gap: 6px;
}

.ping-refresh {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--color-accent);
  cursor: pointer;
}

.ping-refresh:disabled {
  opacity: 0.4;
  cursor: default;
}

.ping-refresh svg {
  width: 14px;
  height: 14px;
}

.place {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  color: var(--color-muted);
  font-size: 12px;
  font-weight: 500;
}
</style>
