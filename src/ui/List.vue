<script setup lang="ts">
defineProps<{
  title?: string;
}>();
</script>

<template>
  <section class="list-group">
    <h2 v-if="title">{{ title }}</h2>
    <ul>
      <slot />
    </ul>
  </section>
</template>

<style scoped>
.list-group {
  margin-bottom: 22px;
}

.list-group h2 {
  margin: 0 0 8px 12px;
  color: var(--color-muted);
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.list-group ul {
  margin: 0;
  padding: 0;
  overflow: hidden;
  list-style: none;
  background: var(--color-surface);
  border-radius: var(--radius-md);
}
</style>
