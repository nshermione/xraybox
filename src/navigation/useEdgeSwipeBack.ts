import { computed, onMounted, onUnmounted, ref, type Ref } from "vue";
import {
  CLAIM_DISTANCE_PX,
  type MotionSample,
  shouldCommitBack,
  shouldStartEdgeSwipe,
  velocityPxPerSecond,
} from "./edgeSwipeBack";
import { useBreakpoint } from "./useBreakpoint";
import { SLIDE_MS, useNavigation } from "./useNavigation";

const BEHIND_SHIFT_PERCENT = 28;
const VERTICAL_CANCEL_PX = 10;
const TAP_SLOP_PX = 6;

export function useEdgeSwipeBack(stackRef: Ref<HTMLElement | null>) {
  const { canGoBack, isAnimating, popImmediate } = useNavigation();
  const { isDesktop } = useBreakpoint();

  const swipeX = ref(0);
  const stackWidth = ref(0);
  const isDragging = ref(false);
  const isSettling = ref(false);

  let pointerId: number | null = null;
  let tracking = false;
  let claimed = false;
  let startX = 0;
  let startY = 0;
  let samples: MotionSample[] = [];
  let settleTimer = 0;

  const swipeProgress = computed(() =>
    stackWidth.value > 0 ? swipeX.value / stackWidth.value : 0,
  );

  function prefersReducedMotion(): boolean {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  function canBegin(): boolean {
    return !isDesktop.value && canGoBack.value && !isAnimating.value && !isSettling.value;
  }

  function bindWindowTracking(): void {
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerCancel);
  }

  function unbindWindowTracking(): void {
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", onPointerUp);
    window.removeEventListener("pointercancel", onPointerCancel);
  }

  function resetTracking(): void {
    tracking = false;
    claimed = false;
    pointerId = null;
    samples = [];
    unbindWindowTracking();
  }

  function forwardTap(event: PointerEvent): void {
    const edge = stackRef.value?.querySelector<HTMLElement>(".edge-back");
    if (!edge) return;

    edge.style.pointerEvents = "none";
    const under = document.elementFromPoint(event.clientX, event.clientY);
    edge.style.pointerEvents = "";

    const target = under?.closest<HTMLElement>("button, a, [role='button']");
    target?.click();
  }

  function onPointerDown(event: PointerEvent): void {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    if (!canBegin() || !stackRef.value) return;

    const rect = stackRef.value.getBoundingClientRect();
    if (!shouldStartEdgeSwipe(event.clientX, rect.left, canGoBack.value)) return;

    tracking = true;
    claimed = false;
    pointerId = event.pointerId;
    startX = event.clientX;
    startY = event.clientY;
    stackWidth.value = rect.width;
    samples = [{ x: event.clientX, t: event.timeStamp }];
    bindWindowTracking();
  }

  function onPointerMove(event: PointerEvent): void {
    if (!tracking || event.pointerId !== pointerId) return;

    const dx = event.clientX - startX;
    const dy = event.clientY - startY;
    samples.push({ x: event.clientX, t: event.timeStamp });
    samples = samples.filter((sample) => event.timeStamp - sample.t <= 100);

    if (!claimed) {
      if (Math.abs(dy) > VERTICAL_CANCEL_PX && Math.abs(dy) > Math.abs(dx)) {
        resetTracking();
        return;
      }

      if (dx < CLAIM_DISTANCE_PX || Math.abs(dx) < Math.abs(dy)) return;

      claimed = true;
      isDragging.value = true;
      isAnimating.value = true;
    }

    swipeX.value = Math.min(stackWidth.value, Math.max(0, dx));
  }

  function finish(commit: boolean): void {
    const duration = prefersReducedMotion() ? 0 : SLIDE_MS;
    const target = commit ? stackWidth.value : 0;

    isDragging.value = false;
    isSettling.value = true;
    isAnimating.value = true;

    if (duration === 0) {
      swipeX.value = 0;
      isSettling.value = false;
      if (commit) {
        popImmediate();
        return;
      }
      isAnimating.value = false;
      return;
    }

    requestAnimationFrame(() => {
      swipeX.value = target;
      settleTimer = window.setTimeout(() => {
        if (commit) popImmediate();
        else isAnimating.value = false;
        swipeX.value = 0;
        isSettling.value = false;
      }, duration);
    });
  }

  function onPointerUp(event: PointerEvent): void {
    if (!tracking || event.pointerId !== pointerId) return;

    const wasClaimed = claimed;
    const dx = event.clientX - startX;
    const dy = event.clientY - startY;
    const progress = swipeProgress.value;
    const velocity = velocityPxPerSecond(samples);
    resetTracking();

    if (!wasClaimed) {
      if (Math.hypot(dx, dy) <= TAP_SLOP_PX) forwardTap(event);
      return;
    }

    finish(shouldCommitBack(progress, velocity));
  }

  function onPointerCancel(event: PointerEvent): void {
    if (!tracking || event.pointerId !== pointerId) return;

    const wasClaimed = claimed;
    resetTracking();
    if (!wasClaimed) return;

    finish(false);
  }

  function onTouchMove(event: TouchEvent): void {
    if (claimed) event.preventDefault();
  }

  function paneStyle(
    index: number,
    visualPaneIndex: number,
  ): { transform: string } | undefined {
    if (swipeX.value <= 0 || stackWidth.value <= 0) return undefined;

    if (index === visualPaneIndex) {
      return { transform: `translate3d(${swipeX.value}px, 0, 0)` };
    }

    if (index === visualPaneIndex - 1) {
      const behindPercent =
        -BEHIND_SHIFT_PERCENT + swipeProgress.value * BEHIND_SHIFT_PERCENT;
      return { transform: `translate3d(${behindPercent}%, 0, 0)` };
    }

    return undefined;
  }

  onMounted(() => {
    const node = stackRef.value;
    if (!node) return;

    node.addEventListener("pointerdown", onPointerDown);
    node.addEventListener("touchmove", onTouchMove, { passive: false });
  });

  onUnmounted(() => {
    window.clearTimeout(settleTimer);
    unbindWindowTracking();
    if (isDragging.value || isSettling.value) {
      isAnimating.value = false;
      swipeX.value = 0;
    }
    const node = stackRef.value;
    if (!node) return;

    node.removeEventListener("pointerdown", onPointerDown);
    node.removeEventListener("touchmove", onTouchMove);
  });

  return {
    swipeX,
    swipeProgress,
    isDragging,
    isSettling,
    paneStyle,
  };
}
