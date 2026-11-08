import { resolvedLocale } from "../state/locale";

const ISO_3166_1_ALPHA_2 = /^[A-Za-z]{2}$/;

export function normalizeCountryCode(code: string): string {
  const normalized = code.trim().toLowerCase();
  return ISO_3166_1_ALPHA_2.test(normalized) ? normalized : "xx";
}

export function countryName(code: string): string {
  const iso = normalizeCountryCode(code).toUpperCase();
  if (iso === "XX") return "";
  try {
    return new Intl.DisplayNames([resolvedLocale.value], { type: "region" }).of(iso) ?? iso;
  } catch {
    return iso;
  }
}
