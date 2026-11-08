<script setup lang="ts">
import { onUnmounted, ref } from "vue";
import { useScrollerBounce } from "./useScrollerBounce";
import { t } from "../state/locale";

const props = defineProps<{
  title?: string;
  parentTitle?: string;
  compact?: boolean;
  fill?: boolean;
  pullRefresh?: boolean;
}>();

const emit = defineEmits<{
  back: [];
  refresh: [];
}>();

const scrollerRef = ref<HTMLElement | null>(null);
const { bounceStyle, pullProgress, isPullingTop } = useScrollerBounce(scrollerRef, {
  enabled: props.pullRefresh === true,
  onPullRefresh: props.pullRefresh ? () => emit("refresh") : undefined,
});

const isScrolling = ref(false);
let hideTimer = 0;

function onScroll() {
  isScrolling.value = true;
  window.clearTimeout(hideTimer);
  hideTimer = window.setTimeout(() => {
    isScrolling.value = false;
  }, 800);
}

onUnmounted(() => {
  window.clearTimeout(hideTimer);
});
</script>

<template>
  <section class="screen">
    <header
      class="header"
      :class="{
        compact,
        'has-back': parentTitle,
        'has-actions': $slots.leading || $slots.trailing,
      }"
    >
      <div v-if="$slots.leading" class="header-slot leading">
        <slot name="leading" />
      </div>
      <button
        v-if="parentTitle"
        type="button"
        class="back"
        @click="emit('back')"
      >
        <span class="back-chevron">‹</span>
        <span>{{ parentTitle }}</span>
      </button>
      <h1 v-if="title">{{ title }}</h1>
      <div v-if="$slots.trailing" class="header-slot trailing">
        <slot name="trailing" />
      </div>
    </header>
    <div
      ref="scrollerRef"
      class="scroller"
      :class="{ fill, 'is-scrolling': isScrolling, 'is-pulling': isPullingTop }"
      @scroll.passive="onScroll"
    >
      <div
        v-if="pullRefresh"
        class="pull-hint"
        :class="{ ready: pullProgress >= 1 }"
        :style="{ opacity: pullProgress, transform: `scale(${0.86 + pullProgress * 0.14})` }"
        aria-hidden="true"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="currentColor"
            d="M4 12a8 8 0 0 1 13.3-6H14v2h6V2h-2v2.6A10 10 0 1 0 22 12h-2a8 8 0 0 1-16 0"
          />
        </svg>
        <span>{{ t("servers.refreshPing") }}</span>
      </div>
      <div class="scroller-content" :style="bounceStyle">
        <slot />
      </div>
    </div>
  </section>
</template>

<style scoped>
.screen {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  background: var(--color-bg);
}

.header {
  flex-shrink: 0;
  padding: calc(12px + env(safe-area-inset-top)) var(--space-xl) 10px;
}

.header.has-actions {
  position: relative;
  min-height: calc(var(--touch) + 8px + env(safe-area-inset-top));
}

.header-slot {
  position: absolute;
  top: calc(8px + env(safe-area-inset-top));
  z-index: 2;
  display: flex;
  align-items: center;
  gap: 2px;
}

.header-slot.leading {
  left: 8px;
}

.header-slot.trailing {
  right: 8px;
}

.header.compact .header-slot,
.header.has-back .header-slot {
  top: env(safe-area-inset-top);
  min-height: 44px;
}

.header-slot :deep(.header-btn) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: var(--touch);
  height: var(--touch);
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--color-accent);
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}

.header-slot :deep(.header-btn:disabled) {
  opacity: 0.4;
}

.header-slot :deep(.header-btn svg) {
  width: 22px;
  height: 22px;
}

.header-slot :deep(.header-text) {
  width: auto;
  padding: 0 12px;
  font-size: 17px;
  font-weight: 500;
}

.header.compact {
  padding-top: calc(8px + env(safe-area-inset-top));
}

.header.has-back {
  position: relative;
  display: flex;
  align-items: center;
  min-height: calc(44px + env(safe-area-inset-top));
  padding-top: env(safe-area-inset-top);
  padding-bottom: 0;
}

.header h1 {
  margin: 0;
  font-size: 32px;
  line-height: 1.2;
}

.header.compact h1 {
  font-size: 22px;
}

.header.has-back h1 {
  position: absolute;
  left: 50%;
  max-width: min(56%, calc(100% - 148px));
  margin: 0;
  overflow: hidden;
  font-size: 17px;
  font-weight: 600;
  text-align: center;
  text-overflow: ellipsis;
  transform: translateX(-50%);
  white-space: nowrap;
}

.back {
  display: inline-flex;
  align-items: center;
  z-index: 1;
  min-height: var(--touch);
  margin: 0 0 0 -10px;
  padding: 0 10px;
  border: 0;
  background: transparent;
  color: var(--color-accent);
  font-size: 17px;
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}

.back-chevron {
  margin-right: 4px;
  font-size: 28px;
  line-height: 1;
  margin-top: -6px;
}

.scroller {
  position: relative;
  flex: 1;
  min-height: 0;
  margin-top: 10px;
  overflow-y: auto;
  overscroll-behavior-y: contain;
  -webkit-overflow-scrolling: touch;
  padding: 0 var(--space-lg) var(--shell-tab-clearance);
}

.scroller.fill {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.scroller.fill .scroller-content {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}

.scroller.is-pulling,
.scroller.is-pulling .scroller-content {
  user-select: none;
  -webkit-user-select: none;
}

.pull-hint {
  position: absolute;
  z-index: 1;
  top: 8px;
  right: 0;
  left: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  pointer-events: none;
  color: var(--color-accent);
  font-size: 13px;
  font-weight: 500;
}

.pull-hint svg {
  width: 16px;
  height: 16px;
}

.pull-hint.ready {
  color: var(--color-accent);
}

@media (hover: hover) and (pointer: fine) {
  .scroller {
    /* margin-right: calc(-1 * var(--scrollbar-size)); */
    /* padding-right: calc(var(--space-lg) + var(--scrollbar-size)); */
    scrollbar-width: thin;
    scrollbar-color: transparent transparent;
  }

  .scroller.is-scrolling {
    scrollbar-color: var(--scrollbar-thumb) transparent;
  }

  .scroller::-webkit-scrollbar {
    width: var(--scrollbar-size);
    height: var(--scrollbar-size);
  }

  .scroller::-webkit-scrollbar-button {
    display: none;
    width: 0;
    height: 0;
  }

  .scroller::-webkit-scrollbar-track,
  .scroller::-webkit-scrollbar-track-piece,
  .scroller::-webkit-scrollbar-corner {
    background: transparent;
  }

  .scroller::-webkit-scrollbar-thumb {
    background: transparent;
    border-radius: var(--radius-pill);
  }

  .scroller.is-scrolling::-webkit-scrollbar-thumb {
    background: var(--scrollbar-thumb);
  }
}
</style>
