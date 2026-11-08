<script setup lang="ts">
import { useBreakpoint } from "../navigation/useBreakpoint";

const { isDesktop } = useBreakpoint();
</script>

<template>
  <div class="app-shell" :class="{ desktop: isDesktop, mobile: !isDesktop }">
    <aside v-if="isDesktop" class="sidebar">
      <p class="brand">xraybox</p>
      <nav class="nav sidebar-nav">
        <slot name="nav" />
      </nav>
    </aside>

    <main class="content">
      <div class="content-inner">
        <slot />
      </div>
    </main>

    <nav v-if="!isDesktop" class="nav bottom-tabs">
      <slot name="nav" />
    </nav>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  height: 100%;
  overflow: hidden;
  background: var(--color-bg);
}

.app-shell.desktop {
  flex-direction: row;
}

.app-shell.mobile {
  position: relative;
  flex-direction: column;
  --shell-tab-clearance: calc(72px + env(safe-area-inset-bottom));
}

.sidebar {
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  width: var(--shell-sidebar-width);
  padding: calc(18px + env(safe-area-inset-top)) 12px 16px;
  border-right: 1px solid var(--color-border);
  background: var(--color-surface);
}

.brand {
  margin: 0 12px 18px;
  color: var(--color-muted);
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.06em;
}

.content {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.content-inner {
  display: flex;
  flex-direction: column;
  flex: 1;
  width: 100%;
  max-width: var(--shell-content-max);
  min-height: 0;
  margin: 0 auto;
}

.nav {
  display: flex;
}

.sidebar-nav {
  flex-direction: column;
  gap: 4px;
}

.bottom-tabs {
  position: absolute;
  z-index: 20;
  right: 20px;
  bottom: calc(12px + env(safe-area-inset-bottom));
  left: 20px;
  justify-content: space-around;
  min-height: 62px;
  padding: 6px 8px 8px;
  border: 0.5px solid var(--tab-border);
  border-radius: 34px;
  background: var(--tab-bg);
  backdrop-filter: blur(var(--tab-blur)) saturate(160%);
  -webkit-backdrop-filter: blur(var(--tab-blur)) saturate(160%);
  box-shadow:
    inset 0 1px 0 var(--tab-inset),
    inset 0 12px 18px -10px var(--tab-glow),
    0 4px 16px rgb(0 0 0 / 4%);
}

@supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px))) {
  .bottom-tabs {
    background: var(--color-surface);
  }
}

.bottom-tabs::before {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  background: linear-gradient(180deg, var(--tab-glow) 0%, transparent 44%);
}

.nav :deep(.nav-item) {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: var(--touch);
  padding: 0 12px;
  border: 0;
  background: transparent;
  color: var(--color-muted);
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}

.nav :deep(.nav-item.active span) {
  margin-top: 3px;
  transition: all 110ms ease;
}

.nav :deep(.nav-item.active) {
  color: var(--color-accent);
}

.sidebar-nav :deep(.nav-item) {
  width: 100%;
  border-radius: var(--radius-sm);
  font-size: 16px;
}

.sidebar-nav :deep(.nav-item.active) {
  background: var(--sidebar-active);
}

.bottom-tabs :deep(.nav-item) {
  position: relative;
  z-index: 1;
  flex: 1;
  flex-direction: column;
  gap: 3px;
  justify-content: center;
  min-height: 50px;
  border-radius: 26px;
  color: var(--color-muted);
  font-size: 10px;
  font-weight: 500;
  letter-spacing: 0.01em;
}

.bottom-tabs :deep(.nav-item.active) {
  color: var(--tab-active);
  /* background: radial-gradient(circle at 50% 36%, var(--tab-lens) 0 17px, transparent 18px); */
}

.nav :deep(.nav-item svg) {
  width: 22px;
  height: 22px;
}
</style>
