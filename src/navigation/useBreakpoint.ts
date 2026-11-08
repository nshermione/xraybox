import { onMounted, onUnmounted, ref } from "vue";
import { isIos } from "../platform";

const DESKTOP_QUERY = "(min-width: 900px)";

const isDesktop = ref(
  typeof window !== "undefined" && !isIos && window.matchMedia(DESKTOP_QUERY).matches,
);

export function useBreakpoint() {
  onMounted(() => {
    const mediaQuery = window.matchMedia(DESKTOP_QUERY);
    const sync = () => {
      isDesktop.value = !isIos && mediaQuery.matches;
    };

    sync();
    mediaQuery.addEventListener("change", sync);
    onUnmounted(() => {
      mediaQuery.removeEventListener("change", sync);
    });
  });

  return { isDesktop };
}
