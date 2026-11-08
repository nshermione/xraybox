<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";

const props = withDefaults(
  defineProps<{
    title: string;
    closable?: boolean;
  }>(),
  {
    closable: true,
  },
);

const emit = defineEmits<{
  close: [];
}>();

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape" && props.closable) emit("close");
}

onMounted(() => window.addEventListener("keydown", onKey));
onUnmounted(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Teleport to="body">
    <div class="backdrop" @click.self="closable ? emit('close') : undefined">
      <div class="panel" role="dialog" aria-modal="true">
        <header>
          <h2>{{ title }}</h2>
          <button
            v-if="closable"
            type="button"
            class="close"
            :aria-label="'Close'"
            @click="emit('close')"
          >
            ×
          </button>
        </header>
        <div class="body">
          <slot />
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgb(0 0 0 / 42%);
}

.panel {
  width: min(420px, 100%);
  max-height: min(80vh, 640px);
  overflow: auto;
  padding: 16px 16px 20px;
  border-radius: var(--radius-md);
  background: var(--color-surface);
  color: var(--color-text);
  box-shadow: 0 16px 40px rgb(0 0 0 / 24%);
}

header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

h2 {
  margin: 0;
  font-size: 18px;
}

.close {
  width: 36px;
  height: 36px;
  border: 0;
  background: transparent;
  color: var(--color-muted);
  font-size: 28px;
  line-height: 1;
  cursor: pointer;
}
</style>
