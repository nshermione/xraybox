<script setup lang="ts">
defineProps<{
  label: string;
  placeholder?: string;
  type?: string;
  disabled?: boolean;
}>();

const model = defineModel<string>({ default: "" });
</script>

<template>
  <label class="field">
    <span>{{ label }}</span>
    <div class="control">
      <input v-model="model" :type="type ?? 'text'" :placeholder="placeholder" :disabled="disabled" />
      <div v-if="$slots.trailing" class="trailing">
        <slot name="trailing" />
      </div>
    </div>
  </label>
</template>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 12px;
}

.field span {
  margin-left: 4px;
  color: var(--color-muted);
  font-size: 13px;
  font-weight: 600;
}

.control {
  position: relative;
}

.field input {
  width: 100%;
  min-height: var(--touch);
  padding: 10px 14px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  background: var(--color-surface);
  color: var(--color-text);
  font-size: 16px;
}

.field input:focus {
  outline: 2px solid var(--color-accent);
  outline-offset: 1px;
}

.trailing {
  position: absolute;
  top: 50%;
  right: 6px;
  display: flex;
  transform: translateY(-50%);
}

.control:has(.trailing) input {
  padding-right: 72px;
}
</style>
