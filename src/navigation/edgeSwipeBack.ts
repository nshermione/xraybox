export const EDGE_ZONE_PX = 28;
export const CLAIM_DISTANCE_PX = 8;
export const COMMIT_RATIO = 0.8;
export const VELOCITY_COMMIT_PX_PER_S = 750;
export const VELOCITY_WINDOW_MS = 80;

export interface MotionSample {
  x: number;
  t: number;
}

export function shouldStartEdgeSwipe(
  clientX: number,
  stackLeft: number,
  canGoBack: boolean,
): boolean {
  return canGoBack && clientX - stackLeft <= EDGE_ZONE_PX;
}

export function velocityPxPerSecond(samples: readonly MotionSample[]): number {
  if (samples.length < 2) return 0;

  const latest = samples[samples.length - 1];
  let oldest = samples[0];

  for (const sample of samples) {
    if (latest.t - sample.t <= VELOCITY_WINDOW_MS) {
      oldest = sample;
      break;
    }
    oldest = sample;
  }

  const elapsedMs = latest.t - oldest.t;
  if (elapsedMs <= 0) return 0;

  return ((latest.x - oldest.x) / elapsedMs) * 1000;
}

export function shouldCommitBack(progress: number, velocityPxPerS: number): boolean {
  return progress >= COMMIT_RATIO || velocityPxPerS >= VELOCITY_COMMIT_PX_PER_S;
}
