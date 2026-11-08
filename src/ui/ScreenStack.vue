<script setup lang="ts">
import type { Component } from "vue";
import { computed, ref } from "vue";
import { useBreakpoint } from "../navigation/useBreakpoint";
import { useEdgeSwipeBack } from "../navigation/useEdgeSwipeBack";
import { useNavigation } from "../navigation/useNavigation";

const props = defineProps<{
  registry: Record<string, Component>;
}>();

const stackRef = ref<HTMLElement | null>(null);
const { currentStack, currentScreen, visualIndex, isAnimating, canGoBack } = useNavigation();
const { isDesktop } = useBreakpoint();
const { isDragging, isSettling, swipeProgress, paneStyle } = useEdgeSwipeBack(stackRef);

const resolvedCurrent = computed(() => props.registry[currentScreen.value.name]);

function paneClass(index: number): string {
  if (index === visualIndex.value) return "is-current";
  if (index === visualIndex.value - 1) return "is-behind";
  if (index < visualIndex.value) return "is-buried";
  return "is-ahead";
}
</script>

<template>
  <div
    ref="stackRef"
    class="stack"
    :class="{ swiping: isDragging, settling: isSettling }"
    :style="{ '--swipe-progress': String(swipeProgress) }"
  >
    <template v-if="!isDesktop">
      <div
        v-for="(screen, index) in currentStack"
        :key="screen.key"
        class="pane"
        :class="[paneClass(index), { animate: isAnimating && !isDragging }]"
        :style="paneStyle(index, visualIndex)"
      >
        <component
          :is="props.registry[screen.name]"
          v-bind="screen.props ?? {}"
        />
      </div>
      <div
        v-if="canGoBack"
        class="edge-back"
        aria-hidden="true"
      />
    </template>
    <Transition v-else name="pane-fade">
      <component
        :is="resolvedCurrent"
        :key="currentScreen.key"
        v-bind="currentScreen.props ?? {}"
      />
    </Transition>
  </div>
</template>

<style scoped>
.stack {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  overscroll-behavior-x: none;
  touch-action: pan-y;
}

.stack.swiping {
  touch-action: none;
  user-select: none;
  -webkit-user-select: none;
}

.pane {
  position: absolute;
  inset: 0;
  transform: translate3d(100%, 0, 0);
  will-change: transform;
  backface-visibility: hidden;
}

.pane.animate {
  transition: transform 320ms cubic-bezier(0.32, 0.72, 0, 1);
}

.pane.is-current {
  z-index: 2;
  transform: translate3d(0, 0, 0);
}

.stack.swiping .pane.is-current,
.stack.settling .pane.is-current {
  box-shadow: -12px 0 28px rgb(0 0 0 / 18%);
}

.stack.swiping .pane.is-behind::after {
  content: "";
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 18%);
  opacity: calc(1 - var(--swipe-progress, 0));
  pointer-events: none;
}

.edge-back {
  position: absolute;
  z-index: 4;
  top: 0;
  bottom: 0;
  left: 0;
  width: 28px;
  touch-action: none;
}

.pane.is-behind {
  z-index: 1;
  transform: translate3d(-28%, 0, 0);
  pointer-events: none;
}

.pane.is-buried {
  z-index: 0;
  transform: translate3d(-100%, 0, 0);
  pointer-events: none;
  visibility: hidden;
}

.pane.is-ahead {
  z-index: 3;
  transform: translate3d(100%, 0, 0);
  pointer-events: none;
}

.pane :deep(.screen) {
  height: 100%;
}

.stack > :deep(.screen),
.stack > :deep(.pane-fade-enter-active),
.stack > :deep(.pane-fade-leave-active) {
  position: absolute;
  inset: 0;
}

.pane-fade-enter-active,
.pane-fade-leave-active {
  transition: opacity 180ms ease;
}

.pane-fade-enter-from,
.pane-fade-leave-to {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .pane.animate,
  .pane-fade-enter-active,
  .pane-fade-leave-active {
    transition: none;
  }
}
</style>
