<script setup lang="ts">
import HelpTip from "./HelpTip.vue";

withDefaults(
  defineProps<{
    title: string;
    subtitle?: string;
    help?: string;
    helpLabel?: string;
    iconLetter?: string;
    iconColor?: string;
    value?: string;
    chevron?: boolean;
    clickable?: boolean;
  }>(),
  {
    chevron: false,
    clickable: false,
  },
);

const emit = defineEmits<{
  click: [];
}>();
</script>

<template>
  <li
    class="item"
    :class="{ clickable }"
    @click="clickable ? emit('click') : undefined"
  >
    <div v-if="$slots.leading" class="leading">
      <slot name="leading" />
    </div>
    <div
      v-else-if="iconLetter"
      class="icon"
      :style="{ background: iconColor ?? 'hsl(210 70% 48%)' }"
    >
      {{ iconLetter }}
    </div>
    <div class="copy">
      <div class="title-row">
        <strong>{{ title }}</strong>
        <HelpTip v-if="help" :text="help" :label="helpLabel || title" />
      </div>
      <span v-if="subtitle">{{ subtitle }}</span>
    </div>
    <slot name="trailing">
      <span v-if="value || $slots.value" class="value">
        <slot name="value">{{ value }}</slot>
      </span>
      <span v-if="chevron" class="chevron">›</span>
    </slot>
  </li>
</template>

<style scoped>
.item {
  display: flex;
  align-items: center;
  gap: var(--space-md);
  min-height: var(--row-min);
  padding: 10px 14px;
  border-bottom: 1px solid var(--color-border);
  -webkit-tap-highlight-color: transparent;
}

.item:last-child {
  border-bottom: 0;
}

.item.clickable {
  cursor: pointer;
}

.item.clickable:active {
  background: var(--color-press);
}

.leading {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
}

.icon {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border-radius: var(--radius-sm);
  color: #fff;
  font-size: 13px;
  font-weight: 700;
}

.copy {
  flex: 1;
  min-width: 0;
}

.title-row {
  display: flex;
  align-items: flex-start;
  gap: 4px;
}

.copy strong,
.copy span {
  display: block;
}

.copy strong {
  min-width: 0;
  font-size: 16px;
  font-weight: 500;
}

.copy span {
  color: var(--color-subtle);
  font-size: 12px;
}

.value,
.chevron {
  color: var(--color-subtle);
  font-size: 16px;
}

.value {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.chevron {
  font-size: 22px;
  line-height: 1;
}
</style>
