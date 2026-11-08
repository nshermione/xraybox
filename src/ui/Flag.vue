<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { countryName } from "../geo/country";
import { loadFlag, peekFlag } from "../geo/flags";

const props = withDefaults(
  defineProps<{
    code: string;
    size?: "xs" | "sm" | "md";
  }>(),
  {
    size: "sm",
  },
);

const src = ref(peekFlag(props.code));
const label = computed(() => countryName(props.code));
let requestId = 0;

watch(
  () => props.code,
  (code) => {
    const cached = peekFlag(code);
    src.value = cached;
    if (cached) return;

    const current = ++requestId;
    void loadFlag(code).then((url) => {
      if (current !== requestId) return;
      src.value = url;
    });
  },
  { immediate: true },
);
</script>

<template>
  <span class="flag" :class="[size, { loaded: Boolean(src) }]" role="img" :aria-label="label || code">
    <img v-if="src" :src="src" alt="" />
  </span>
</template>

<style scoped>
.flag {
  display: block;
  flex-shrink: 0;
  overflow: hidden;
  background: var(--color-press);
  border-radius: 3px;
  box-shadow: inset 0 0 0 1px rgb(0 0 0 / 12%);
}

.flag img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.flag.xs {
  width: 16px;
  height: 12px;
}

.flag.sm {
  width: 22px;
  height: 16px;
}

.flag.md {
  width: 28px;
  height: 21px;
}

.flag.loaded {
  background: transparent;
}

[data-theme="dark"] .flag {
  box-shadow: inset 0 0 0 1px rgb(255 255 255 / 16%);
}
</style>
