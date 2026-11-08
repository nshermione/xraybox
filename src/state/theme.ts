import { computed, ref } from "vue";
import { t } from "./locale";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

export const THEME_OPTION_IDS: ThemePreference[] = ["system", "light", "dark"];

const STORAGE_KEY = "xraybox.theme";

function readStored(): ThemePreference {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "light" || stored === "dark" || stored === "system") return stored;
  return "system";
}

function systemIsDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export const themePreference = ref<ThemePreference>("system");
export const resolvedTheme = ref<ResolvedTheme>("light");
export const themeLabel = computed(() => t(`theme.options.${themePreference.value}.title`));

function applyTheme(): void {
  const isDark = themePreference.value === "dark" || (themePreference.value === "system" && systemIsDark());
  resolvedTheme.value = isDark ? "dark" : "light";
  document.documentElement.dataset.theme = resolvedTheme.value;
  document.documentElement.style.colorScheme = resolvedTheme.value;
}

export function setThemePreference(next: ThemePreference): void {
  themePreference.value = next;
  localStorage.setItem(STORAGE_KEY, next);
  applyTheme();
}

export function initTheme(): void {
  themePreference.value = readStored();
  applyTheme();
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applyTheme);
}
