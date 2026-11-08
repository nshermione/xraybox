<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref } from "vue";

const props = defineProps<{
  text: string;
  label: string;
}>();

const paragraphs = computed(() =>
  props.text
    .split(/\n\n+/)
    .map((part) => part.trim())
    .filter(Boolean),
);

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const panel = ref<HTMLElement | null>(null);
const panelId = `help-tip-${Math.random().toString(36).slice(2, 9)}`;
const panelStyle = ref<Record<string, string>>({});

function placePanel(): void {
  const button = root.value;
  const bubble = panel.value;
  if (!button || !bubble) return;

  const rect = button.getBoundingClientRect();
  const size = bubble.getBoundingClientRect();
  const margin = 12;
  const bottomSafe = 88;
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;
  let top = rect.bottom + 8;
  if (top + size.height > viewportHeight - bottomSafe) {
    top = rect.top - size.height - 8;
  }
  top = Math.max(margin, Math.min(top, viewportHeight - size.height - margin));

  let left = rect.left + rect.width / 2 - size.width / 2;
  left = Math.max(margin, Math.min(left, viewportWidth - size.width - margin));

  panelStyle.value = {
    top: `${Math.round(top)}px`,
    left: `${Math.round(left)}px`,
  };
}

async function setOpen(next: boolean): Promise<void> {
  open.value = next;
  if (!next) return;
  await nextTick();
  requestAnimationFrame(() => {
    if (open.value) placePanel();
  });
}

function toggle(event: Event): void {
  event.stopPropagation();
  void setOpen(!open.value);
}

function onPointerDown(event: PointerEvent): void {
  if (!open.value) return;
  const target = event.target as Node | null;
  if (target && (root.value?.contains(target) || panel.value?.contains(target))) return;
  void setOpen(false);
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape" && open.value) void setOpen(false);
}

function dismiss(): void {
  if (open.value) void setOpen(false);
}

document.addEventListener("pointerdown", onPointerDown);
document.addEventListener("keydown", onKeydown);
window.addEventListener("resize", dismiss);
window.addEventListener("scroll", dismiss, true);

onUnmounted(() => {
  document.removeEventListener("pointerdown", onPointerDown);
  document.removeEventListener("keydown", onKeydown);
  window.removeEventListener("resize", dismiss);
  window.removeEventListener("scroll", dismiss, true);
});
</script>

<template>
  <span ref="root" class="help-tip">
    <button
      type="button"
      class="help-btn"
      :aria-label="props.label"
      :aria-expanded="open"
      :aria-controls="panelId"
      @click="toggle"
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="8.25" fill="none" stroke="currentColor" stroke-width="1.8" />
        <path
          d="M9.7 9.5a2.4 2.4 0 1 1 3.4 2.2c-.7.4-1.1 1-1.1 1.8"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
        />
        <circle cx="12" cy="16.6" r="1.05" fill="currentColor" />
      </svg>
    </button>
    <Teleport to="body">
      <div
        v-if="open"
        :id="panelId"
        ref="panel"
        class="help-panel"
        role="tooltip"
        :style="panelStyle"
      >
        <p v-for="(paragraph, index) in paragraphs" :key="index">{{ paragraph }}</p>
      </div>
    </Teleport>
  </span>
</template>

<style scoped>
.help-tip {
  display: inline-flex;
  flex-shrink: 0;
  align-items: center;
}

.help-btn {
  display: grid;
  place-items: center;
  width: var(--touch);
  height: var(--touch);
  margin: -12px -6px -12px 0;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--color-subtle);
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}

.help-btn:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: -6px;
  border-radius: 50%;
}

.help-btn:active,
.help-btn[aria-expanded="true"] {
  color: var(--color-accent);
}

.help-btn svg {
  width: 18px;
  height: 18px;
}

.help-panel {
  position: fixed;
  z-index: 200;
  width: min(320px, calc(100vw - 24px));
  max-height: min(360px, calc(100vh - 120px));
  margin: 0;
  padding: 10px 12px;
  overflow-y: auto;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-surface);
  box-shadow: 0 10px 28px rgb(0 0 0 / 18%);
  color: var(--color-muted);
  font-size: 13px;
  line-height: 1.45;
}

.help-panel p {
  margin: 0;
}

.help-panel p + p {
  margin-top: 8px;
}
</style>
