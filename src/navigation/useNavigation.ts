import { computed, nextTick, ref } from "vue";

export type TabId = "home" | "settings";
export type ScreenName =
  | TabId
  | "servers"
  | "nested-setting"
  | "appearance"
  | "language"
  | "routing"
  | "route-advanced"
  | "fetch-interval"
  | "logs";

export interface ScreenEntry {
  key: number;
  name: ScreenName;
  title: string;
  parentTitle?: string;
  props?: Record<string, unknown>;
}

export const TABS: { id: TabId }[] = [
  { id: "home" },
  { id: "settings" },
];

export const SLIDE_MS = 320;

let nextKey = 1;

function rootScreen(tab: TabId, title: string): ScreenEntry {
  return { key: nextKey++, name: tab, title };
}

function shouldSlide(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia("(max-width: 899px)").matches &&
    !window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

const tab = ref<TabId>("home");
const stacks = {
  home: ref<ScreenEntry[]>([rootScreen("home", "Home")]),
  settings: ref<ScreenEntry[]>([rootScreen("settings", "Settings")]),
};
const visualIndexes = {
  home: ref(0),
  settings: ref(0),
};
const isAnimating = ref(false);

const currentStack = computed(() => stacks[tab.value].value);
const visualIndex = computed(() => visualIndexes[tab.value].value);
const currentScreen = computed(() => currentStack.value[visualIndex.value] ?? currentStack.value[0]);
const canGoBack = computed(() => visualIndex.value > 0);

function syncIndex(id: TabId): void {
  visualIndexes[id].value = Math.max(0, stacks[id].value.length - 1);
}

function popToRoot(id: TabId): void {
  const stack = stacks[id];
  if (stack.value.length > 1) {
    stack.value = [stack.value[0]];
  }
  visualIndexes[id].value = 0;
  isAnimating.value = false;
}

function switchTab(next: TabId): void {
  if (isAnimating.value) return;
  popToRoot(next);
  tab.value = next;
}

function popImmediate(): void {
  const id = tab.value;
  if (visualIndexes[id].value <= 0) return;

  stacks[id].value.pop();
  syncIndex(id);
  isAnimating.value = false;
}

async function push(entry: Omit<ScreenEntry, "key">): Promise<void> {
  if (isAnimating.value) return;

  const id = tab.value;
  stacks[id].value.push({ ...entry, key: nextKey++ });

  if (!shouldSlide()) {
    syncIndex(id);
    return;
  }

  isAnimating.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    visualIndexes[id].value += 1;
    window.setTimeout(() => {
      isAnimating.value = false;
    }, SLIDE_MS);
  });
}

function back(): void {
  const id = tab.value;
  if (isAnimating.value || visualIndexes[id].value <= 0) return;

  if (!shouldSlide()) {
    stacks[id].value.pop();
    syncIndex(id);
    return;
  }

  isAnimating.value = true;
  visualIndexes[id].value -= 1;
  window.setTimeout(() => {
    stacks[id].value.pop();
    isAnimating.value = false;
  }, SLIDE_MS);
}

export function useNavigation() {
  return {
    tab,
    stacks,
    currentStack,
    currentScreen,
    visualIndex,
    canGoBack,
    isAnimating,
    switchTab,
    popToRoot,
    push,
    back,
    popImmediate,
  };
}
