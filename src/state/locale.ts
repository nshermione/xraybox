import { computed, ref } from "vue";
import en from "../i18n/locales/en.json";
import vi from "../i18n/locales/vi.json";

export type LocalePreference = "system" | "vi" | "en";
export type ResolvedLocale = "vi" | "en";

export const LOCALE_OPTION_IDS: LocalePreference[] = ["system", "vi", "en"];

const STORAGE_KEY = "xraybox.locale";

type MessageTree = { [key: string]: string | MessageTree };

const catalogs = { vi, en } as Record<ResolvedLocale, MessageTree>;

function readStored(): LocalePreference {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "vi" || stored === "en" || stored === "system") return stored;
  return "system";
}

function systemLocale(): ResolvedLocale {
  const languages = navigator.languages?.length ? navigator.languages : [navigator.language];
  const primary = (languages[0] ?? "").toLowerCase();
  return primary.startsWith("en") ? "en" : "vi";
}

function lookup(tree: MessageTree, path: string): string | undefined {
  let current: string | MessageTree | undefined = tree;
  for (const part of path.split(".")) {
    if (current == null || typeof current === "string") return undefined;
    current = current[part];
  }
  return typeof current === "string" ? current : undefined;
}

export const localePreference = ref<LocalePreference>("system");
export const resolvedLocale = ref<ResolvedLocale>("vi");
export const localeLabel = computed(() => t(`language.options.${localePreference.value}.title`));

export function t(key: string, params?: Record<string, string>): string {
  const value =
    lookup(catalogs[resolvedLocale.value], key) ?? lookup(catalogs.vi, key) ?? key;
  if (!params) return value;
  return Object.entries(params).reduce(
    (text, [name, replacement]) => text.split(`{${name}}`).join(replacement),
    value,
  );
}

function applyLocale(): void {
  resolvedLocale.value =
    localePreference.value === "system" ? systemLocale() : localePreference.value;
  document.documentElement.lang = resolvedLocale.value;
}

export function setLocalePreference(next: LocalePreference): void {
  localePreference.value = next;
  localStorage.setItem(STORAGE_KEY, next);
  applyLocale();
}

export function initLocale(): void {
  localePreference.value = readStored();
  applyLocale();
  window.addEventListener("languagechange", applyLocale);
}
