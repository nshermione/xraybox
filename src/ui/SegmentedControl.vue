<script setup lang="ts">
export interface SegmentOption {
  id: string;
  label: string;
}

defineProps<{
  options: SegmentOption[];
}>();

const model = defineModel<string>({ required: true });
</script>

<template>
  <div class="segments" role="tablist">
    <button
      v-for="option in options"
      :key="option.id"
      type="button"
      role="tab"
      :aria-selected="model === option.id"
      :class="{ active: model === option.id }"
      @click="model = option.id"
    >
      {{ option.label }}
    </button>
  </div>
</template>

<style scoped>
.segments {
  display: flex;
  padding: 3px;
  overflow: hidden;
  background: var(--color-press);
  border-radius: var(--radius-md);
}

.segments button {
  flex: 1;
  min-height: 36px;
  padding: 0 12px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: var(--color-muted);
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
}

.segments button.active {
  background: var(--color-surface);
  color: var(--color-text);
  box-shadow: 0 1px 2px rgb(0 0 0 / 8%);
}
</style>
