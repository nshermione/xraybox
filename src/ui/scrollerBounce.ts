import { EDGE_ZONE_PX } from "../navigation/edgeSwipeBack";

export const BOUNCE_CLAIM_PX = 4;
export const BOUNCE_RELEASE_MS = 420;
export const BOUNCE_WHEEL_IDLE_MS = 90;
export const PULL_REFRESH_PX = 64;
export const RUBBER_CONSTANT = 0.35;

export type ScrollEdge = "top" | "bottom" | "none";

export function rubberBandOffset(overscrollPx: number, dimensionPx: number): number {
  if (overscrollPx <= 0 || dimensionPx <= 0) return 0;

  return (
    (1 - 1 / ((overscrollPx * RUBBER_CONSTANT) / dimensionPx + 1)) * dimensionPx
  );
}

export function hasScrollOverflow(
  clientHeight: number,
  scrollHeight: number,
  epsilon = 1,
): boolean {
  return scrollHeight > clientHeight + epsilon;
}

export function scrollEdge(
  scrollTop: number,
  clientHeight: number,
  scrollHeight: number,
  epsilon = 1,
): ScrollEdge {
  if (!hasScrollOverflow(clientHeight, scrollHeight, epsilon)) return "none";

  const atTop = scrollTop <= epsilon;
  const atBottom = scrollTop + clientHeight >= scrollHeight - epsilon;

  if (atTop) return "top";
  if (atBottom) return "bottom";
  return "none";
}

export function bouncePullPx(edge: ScrollEdge, deltaY: number): number {
  if (edge === "top") return Math.max(0, deltaY);
  if (edge === "bottom") return Math.min(0, deltaY);
  return 0;
}

export function shouldIgnoreBounceStart(clientX: number, containerLeft: number): boolean {
  return clientX - containerLeft <= EDGE_ZONE_PX;
}

export function signedRubberBand(rawOverscrollPx: number, dimensionPx: number): number {
  if (rawOverscrollPx === 0) return 0;

  return (
    Math.sign(rawOverscrollPx) * rubberBandOffset(Math.abs(rawOverscrollPx), dimensionPx)
  );
}

export function wheelDeltaY(event: WheelEvent): number {
  if (event.deltaMode === WheelEvent.DOM_DELTA_LINE) return event.deltaY * 16;
  if (event.deltaMode === WheelEvent.DOM_DELTA_PAGE) {
    return event.deltaY * (event.currentTarget instanceof HTMLElement
      ? event.currentTarget.clientHeight
      : window.innerHeight);
  }

  return event.deltaY;
}

export function nextWheelOverscroll(
  rawOverscroll: number,
  edge: ScrollEdge,
  deltaY: number,
): number | null {
  const nextRaw = rawOverscroll - deltaY;
  const pullingTop = nextRaw > 0;
  const pullingBottom = nextRaw < 0;
  const continuing = rawOverscroll !== 0;
  const startingTop = edge === "top" && pullingTop;
  const startingBottom = edge === "bottom" && pullingBottom;

  if (!continuing && !startingTop && !startingBottom) return null;

  if ((pullingTop && edge === "bottom") || (pullingBottom && edge === "top")) {
    return 0;
  }

  return nextRaw;
}
