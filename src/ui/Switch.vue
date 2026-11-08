<script setup lang="ts">
defineProps<{
  disabled?: boolean;
}>();

const checked = defineModel<boolean>({ default: false });
</script>

<template>
  <label class="switch" :class="{ disabled }" @click.stop>
    <input v-model="checked" type="checkbox" :disabled="disabled" />
    <span />
  </label>
</template>

<style scoped>
.switch {
  position: relative;
  flex-shrink: 0;
  width: 46px;
  height: 28px;
  cursor: pointer;
}

.switch input {
  position: absolute;
  opacity: 0;
}

.switch span {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: var(--radius-pill);
  background: var(--color-switch-off);
  border: 1px solid var(--color-press);
}

.switch span::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: var(--color-surface);
  box-shadow: 0 1px 2px rgb(0 0 0 / 20%);
}

.switch input:checked + span {
  background: var(--color-accent);
}

.switch input:checked + span::after {
  left: 20px;
}

.switch input:focus-visible + span {
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
}

.switch.disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
