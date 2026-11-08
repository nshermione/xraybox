<script setup lang="ts">
import { computed } from "vue";
import { t } from "../../state/locale";
import type { ConnectionStatus } from "../../state/vpn";

const props = defineProps<{
  status: ConnectionStatus;
}>();

const emit = defineEmits<{
  click: [];
}>();

const labels = computed<Record<ConnectionStatus, string>>(() => ({
  disconnected: t("vpn.connect"),
  connecting: t("vpn.connecting"),
  connected: t("vpn.disconnect"),
  disconnecting: t("vpn.disconnecting"),
}));

const busy = computed(() => props.status === "connecting" || props.status === "disconnecting");
</script>

<template>
  <button
    type="button"
    class="connect"
    :class="status"
    :aria-label="labels[status]"
    :aria-busy="busy"
    @click="emit('click')"
  >
    <span class="glow" aria-hidden="true" />
    <span class="spin" aria-hidden="true" />
    <span class="core">
      <span class="specular" aria-hidden="true" />
      <span class="caustic" aria-hidden="true" />
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path
          d="M12 3v8"
          fill="none"
          stroke="currentColor"
          stroke-width="2.2"
          stroke-linecap="round"
        />
        <path
          d="M7.2 6.4a7.2 7.2 0 1 0 9.6 0"
          fill="none"
          stroke="currentColor"
          stroke-width="2.2"
          stroke-linecap="round"
        />
      </svg>
    </span>
  </button>
</template>

<style scoped>
.connect {
  position: relative;
  width: 124px;
  height: 124px;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: pointer;
  isolation: isolate;
  -webkit-tap-highlight-color: transparent;
}

.glow {
  position: absolute;
  inset: -42px;
  border-radius: 50%;
  background: radial-gradient(
    circle,
    rgb(48 209 88 / 42%) 0%,
    rgb(48 209 88 / 16%) 38%,
    transparent 68%
  );
  opacity: 0;
  pointer-events: none;
  filter: blur(12px);
  transition: opacity 240ms ease;
}

.rim {
  position: absolute;
  inset: -3px;
  border-radius: 50%;
  background: conic-gradient(
    from 210deg,
    rgb(255 255 255 / 0%) 0deg,
    rgb(255 255 255 / 70%) 70deg,
    rgb(255 255 255 / 10%) 150deg,
    rgb(255 255 255 / 55%) 250deg,
    rgb(255 255 255 / 0%) 360deg
  );
  opacity: 0.85;
  pointer-events: none;
  mask: radial-gradient(farthest-side, transparent calc(100% - 1.5px), #000 calc(100% - 1px));
  -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 1.5px), #000 calc(100% - 1px));
}

.core {
  position: relative;
  z-index: 1;
  display: grid;
  place-items: center;
  width: 100%;
  height: 100%;
  overflow: hidden;
  border: 0.5px solid var(--glass-border);
  border-radius: 50%;
  background:
    linear-gradient(165deg, rgb(255 255 255 / 52%) 0%, rgb(255 255 255 / 16%) 46%, rgb(236 236 240 / 34%) 100%);
  color: var(--connect-off-fg);
  box-shadow:
    inset 0 0.5px 0 var(--glass-highlight),
    inset 0 -1px 0 var(--glass-edge),
    0 10px 30px rgb(0 0 0 / 10%);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate)) brightness(1.06);
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate)) brightness(1.06);
  transition: background 240ms ease, color 240ms ease, box-shadow 240ms ease;
}

.specular {
  display: none;
  position: absolute;
  top: 6%;
  left: 16%;
  width: 68%;
  height: 38%;
  border-radius: 50%;
  background: linear-gradient(180deg, rgb(255 255 255 / 58%), rgb(255 255 255 / 0%));
  pointer-events: none;
}

.caustic {
  display: none;
  position: absolute;
  right: 14%;
  bottom: 16%;
  width: 28%;
  height: 18%;
  border-radius: 50%;
  background: rgb(255 255 255 / 28%);
  filter: blur(4px);
  pointer-events: none;
}

.core svg {
  position: relative;
  z-index: 1;
  width: 44px;
  height: 44px;
}

.spin {
  position: absolute;
  inset: -6px;
  border-radius: 50%;
  background: conic-gradient(from 90deg, transparent 0 68%, currentColor 100%);
  color: transparent;
  opacity: 0;
  pointer-events: none;
  mask: radial-gradient(farthest-side, transparent calc(100% - 2.5px), #000 calc(100% - 1.5px));
  -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 2.5px), #000 calc(100% - 1.5px));
}

.connect.connecting {
  cursor: pointer;
}

.connect.disconnecting {
  cursor: progress;
}

.connect.connecting .core {
  background:
    linear-gradient(165deg, rgb(212, 233, 255) 0%, rgb(55, 152, 255) 100%);
  color: #fff;
}

.connect.connecting .glow {
  opacity: 0.85;
  background: radial-gradient(
    circle,
    rgb(0 122 255 / 40%) 0%,
    rgb(0 122 255 / 14%) 38%,
    transparent 68%
  );
  animation: pulse 1.1s ease-out infinite;
}

.connect.connecting .spin {
  color: rgb(55 152 255);
  opacity: 1;
  animation: spin 0.9s linear infinite;
}

.connect.connecting .rim {
  background: conic-gradient(
    from 180deg,
    rgb(0 122 255 / 0%) 0deg,
    rgb(160 210 255 / 80%) 90deg,
    rgb(0 122 255 / 10%) 180deg,
    rgb(160 210 255 / 70%) 280deg,
    rgb(0 122 255 / 0%) 360deg
  );
  animation: pulse 1.1s ease-out infinite;
}

.connect.disconnecting .core {
  background:
    linear-gradient(165deg, rgb(255 224 178) 0%, rgb(255 149 0) 100%);
  color: #fff;
}

.connect.disconnecting .glow {
  opacity: 0.85;
  background: radial-gradient(
    circle,
    rgb(255 149 0 / 42%) 0%,
    rgb(255 149 0 / 16%) 38%,
    transparent 68%
  );
  animation: pulse 1.1s ease-out infinite;
}

.connect.disconnecting .spin {
  color: rgb(255 149 0);
  opacity: 1;
  animation: spin 0.9s linear infinite;
}

.connect.connected .glow {
  opacity: 1;
}

.connect.connected .core {
  border: none;
  background:
    linear-gradient(160deg, rgb(196 255 214 / 62%) 0%, rgb(99 245 135 / 80%) 52%, rgb(44 205 87 / 86%) 100%);
  color: #fff;
    box-shadow: inset 0 0.5px 0 rgb(255 255 255 / 72%), inset 0 -1px 0 rgb(0 80 30 / 12%), 0 0 16px rgb(137 255 167 / 40%), 0 0 40px rgb(110 255 146 / 26%), 0 12px 32px rgba(85, 231, 122, 0.16);
}

.connect.connected .rim {
  background: conic-gradient(
    from 200deg,
    rgb(255 255 255 / 0%) 0deg,
    rgb(220 255 230 / 85%) 80deg,
    rgb(48 209 88 / 15%) 160deg,
    rgb(255 255 255 / 70%) 260deg,
    rgb(255 255 255 / 0%) 360deg
  );
}

:global(html[data-theme="dark"]) .core {
  background:
    linear-gradient(165deg, rgb(255 255 255 / 16%) 0%, rgb(255 255 255 / 5%) 46%, rgb(40 40 44 / 48%) 100%);
}

@supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px))) {
  .core {
    background: var(--glass-fill-strong);
  }
}

@media (prefers-reduced-motion: reduce) {
  .glow {
    transition: none;
  }

  .connect.connecting .rim,
  .connect.connecting .glow,
  .connect.connecting .spin,
  .connect.disconnecting .glow,
  .connect.disconnecting .spin {
    animation: none;
  }
}

@keyframes pulse {
  0% {
    transform: scale(0.96);
    opacity: 0.85;
  }
  100% {
    transform: scale(1.08);
    opacity: 0;
  }
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
