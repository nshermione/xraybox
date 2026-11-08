<script setup lang="ts">
withDefaults(
  defineProps<{
    variant?: "primary" | "secondary" | "destructive";
    type?: "button" | "submit";
    disabled?: boolean;
  }>(),
  {
    variant: "primary",
    type: "button",
  },
);
</script>

<template>
  <button :type="type" class="btn" :class="variant" :disabled="disabled">
    <slot />
  </button>
</template>

<style scoped>
.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  width: 100%;
  min-height: var(--touch);
  padding: 0 16px;
  border: 0;
  border-radius: var(--radius-md);
  font-size: 16px;
  font-weight: 600;
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}

.btn.primary {
  background: var(--color-accent);
  color: #fff;
}

.btn.secondary {
  background: var(--color-surface);
  color: var(--color-accent);
}

.btn.destructive {
  background: var(--color-surface);
  color: var(--color-danger);
}

.btn:active {
  opacity: 0.82;
}

.btn:disabled {
  opacity: 0.45;
  cursor: default;
}

.btn :deep(svg) {
  width: 18px;
  height: 18px;
  flex-shrink: 0;
}
</style>
