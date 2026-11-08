<script setup lang="ts">
defineProps<{
  title: string;
  message?: string;
}>();
</script>

<template>
  <div class="empty">
    <strong>{{ title }}</strong>
    <p v-if="message">{{ message }}</p>
    <slot />
  </div>
</template>

<style scoped>
.empty {
  padding: 36px 16px;
  color: var(--color-muted);
  text-align: center;
}

.empty strong {
  display: block;
  color: var(--color-text);
  font-size: 17px;
  font-weight: 600;
}

.empty p {
  margin: 8px 0 0;
  font-size: 14px;
  line-height: 1.5;
}
</style>
