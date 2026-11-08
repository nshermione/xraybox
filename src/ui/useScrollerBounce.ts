import { computed, onMounted, onUnmounted, ref, type Ref } from "vue";
import {
  BOUNCE_CLAIM_PX,
  BOUNCE_RELEASE_MS,
  BOUNCE_WHEEL_IDLE_MS,
  PULL_REFRESH_PX,
  bouncePullPx,
  nextWheelOverscroll,
  scrollEdge,
  shouldIgnoreBounceStart,
  signedRubberBand,
  wheelDeltaY,
  type ScrollEdge,
} from "./scrollerBounce";

export function useScrollerBounce(
  scrollerRef: Ref<HTMLElement | null>,
  options?: {
    enabled?: boolean;
    onPullRefresh?: () => void;
    refreshThreshold?: number;
  },
) {
  const offset = ref(0);
  const isReleasing = ref(false);
  const topPull = ref(0);
  const topPullRaw = ref(0);

  let rawOverscroll = 0;
  let pointerId: number | null = null;
  let tracking = false;
  let ignored = false;
  let claimed = false;
  let startX = 0;
  let startY = 0;
  let lastY = 0;
  let pullOriginY = 0;
  let lockedEdge: ScrollEdge = "none";
  let edgeOnDown: ScrollEdge = "none";
  let releaseTimer = 0;
  let wheelIdleTimer = 0;
  let peakTopPull = 0;

  const isBouncing = computed(() => offset.value !== 0 || isReleasing.value);

  const bounceStyle = computed(() => {
    if (options?.enabled === false || !isBouncing.value) return undefined;

    return {
      transform: `translate3d(0, ${offset.value}px, 0)`,
      transition: isReleasing.value
        ? `transform ${BOUNCE_RELEASE_MS}ms cubic-bezier(0.22, 1, 0.36, 1)`
        : "none",
    };
  });

  const pullProgress = computed(() => {
    const threshold = options?.refreshThreshold ?? PULL_REFRESH_PX;
    return Math.min(1, Math.max(0, topPullRaw.value) / threshold);
  });

  const isPullingTop = computed(() => topPull.value > 0);

  function prefersReducedMotion(): boolean {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  function applyOffset(dimensionPx: number): void {
    if (rawOverscroll > 0) {
      peakTopPull = Math.max(peakTopPull, rawOverscroll);
    }
    offset.value = signedRubberBand(rawOverscroll, dimensionPx);
    topPull.value = Math.max(0, offset.value);
    topPullRaw.value = Math.max(0, rawOverscroll);
  }

  function clearSelection(): void {
    window.getSelection()?.removeAllRanges();
  }

  function currentEdge(node: HTMLElement): ScrollEdge {
    const edge = scrollEdge(node.scrollTop, node.clientHeight, node.scrollHeight);
    if (edge !== "none" || !options?.onPullRefresh) return edge;
    return node.scrollTop <= 1 ? "top" : "none";
  }

  function cancelRelease(): void {
    window.clearTimeout(releaseTimer);
    isReleasing.value = false;
  }

  function release(): void {
    const shouldRefresh = peakTopPull >= (options?.refreshThreshold ?? PULL_REFRESH_PX);
    peakTopPull = 0;
    rawOverscroll = 0;
    window.clearTimeout(wheelIdleTimer);

    if (shouldRefresh) {
      options?.onPullRefresh?.();
    }

    if (offset.value === 0 || prefersReducedMotion()) {
      offset.value = 0;
      topPull.value = 0;
      topPullRaw.value = 0;
      isReleasing.value = false;
      return;
    }

    isReleasing.value = true;
    offset.value = 0;
    topPull.value = 0;
    topPullRaw.value = 0;
    window.clearTimeout(releaseTimer);
    releaseTimer = window.setTimeout(() => {
      isReleasing.value = false;
    }, BOUNCE_RELEASE_MS);
  }

  function onSelectStart(event: Event): void {
    if (claimed || isPullingTop.value) event.preventDefault();
  }

  function bindWindowPointer(): void {
    window.addEventListener("pointermove", onPointerMove, { passive: false });
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerCancel);
    window.addEventListener("selectstart", onSelectStart);
  }

  function unbindWindowPointer(): void {
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", onPointerUp);
    window.removeEventListener("pointercancel", onPointerCancel);
    window.removeEventListener("selectstart", onSelectStart);
  }

  function resetPointer(): void {
    tracking = false;
    ignored = false;
    claimed = false;
    pointerId = null;
    lockedEdge = "none";
    edgeOnDown = "none";
    unbindWindowPointer();
  }

  function onPointerDown(event: PointerEvent): void {
    if (event.pointerType === "mouse" && !options?.onPullRefresh) return;
    if (prefersReducedMotion()) return;

    const node = scrollerRef.value;
    if (!node) return;

    const rect = node.getBoundingClientRect();
    if (shouldIgnoreBounceStart(event.clientX, rect.left)) return;

    cancelRelease();
    tracking = true;
    ignored = false;
    claimed = rawOverscroll !== 0;
    pointerId = event.pointerId;
    startX = event.clientX;
    startY = event.clientY;
    lastY = event.clientY;
    peakTopPull = 0;
    edgeOnDown = currentEdge(node);
    if (claimed) {
      lockedEdge = rawOverscroll > 0 ? "top" : "bottom";
      pullOriginY = event.clientY - rawOverscroll;
    }
    bindWindowPointer();
  }

  function onPointerMove(event: PointerEvent): void {
    if (!tracking || ignored || event.pointerId !== pointerId) return;

    const node = scrollerRef.value;
    if (!node) return;

    const dx = event.clientX - startX;
    const fromStartY = event.clientY - startY;
    const frameDy = event.clientY - lastY;
    lastY = event.clientY;

    if (claimed) {
      event.preventDefault();
      clearSelection();
    }

    if (!claimed) {
      if (Math.abs(dx) > 10 && Math.abs(dx) > Math.abs(fromStartY)) {
        ignored = true;
        return;
      }

      const edge = currentEdge(node);
      const startedAtEdge = edgeOnDown !== "none";
      const pull = startedAtEdge
        ? bouncePullPx(edgeOnDown, fromStartY)
        : bouncePullPx(edge, frameDy);
      const claimPx = startedAtEdge ? BOUNCE_CLAIM_PX : 1;
      if (Math.abs(pull) < claimPx) return;

      claimed = true;
      lockedEdge = startedAtEdge ? edgeOnDown : edge;
      rawOverscroll = pull;
      pullOriginY = event.clientY - pull;
      clearSelection();
      applyOffset(node.clientHeight);
      return;
    }

    rawOverscroll = bouncePullPx(lockedEdge, event.clientY - pullOriginY);
    applyOffset(node.clientHeight);
  }

  function onPointerUp(event: PointerEvent): void {
    if (!tracking || event.pointerId !== pointerId) return;

    const shouldRelease = claimed || rawOverscroll !== 0;
    resetPointer();
    if (shouldRelease) release();
  }

  function onPointerCancel(event: PointerEvent): void {
    if (!tracking || event.pointerId !== pointerId) return;

    const shouldRelease = claimed || rawOverscroll !== 0;
    resetPointer();
    if (shouldRelease) release();
  }

  function onTouchMove(event: TouchEvent): void {
    if (claimed) event.preventDefault();
  }

  function onWheel(event: WheelEvent): void {
    if (claimed || prefersReducedMotion()) return;

    const node = scrollerRef.value;
    if (!node) return;

    const edge = currentEdge(node);
    const next = nextWheelOverscroll(rawOverscroll, edge, wheelDeltaY(event));
    if (next === null) return;

    event.preventDefault();
    cancelRelease();
    rawOverscroll = next;
    applyOffset(node.clientHeight);

    window.clearTimeout(wheelIdleTimer);
    if (rawOverscroll === 0) {
      offset.value = 0;
      return;
    }

    wheelIdleTimer = window.setTimeout(release, BOUNCE_WHEEL_IDLE_MS);
  }

  onMounted(() => {
    if (options?.enabled === false) return;

    const node = scrollerRef.value;
    if (!node) return;

    node.addEventListener("pointerdown", onPointerDown);
    node.addEventListener("touchmove", onTouchMove, { passive: false });
    node.addEventListener("wheel", onWheel, { passive: false });
  });

  onUnmounted(() => {
    window.clearTimeout(releaseTimer);
    window.clearTimeout(wheelIdleTimer);
    unbindWindowPointer();
    const node = scrollerRef.value;
    if (!node) return;

    node.removeEventListener("pointerdown", onPointerDown);
    node.removeEventListener("touchmove", onTouchMove);
    node.removeEventListener("wheel", onWheel);
  });

  return {
    bounceStyle,
    isBouncing,
    pullProgress,
    isPullingTop,
  };
}
