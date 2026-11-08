/// <reference types="vite/client" />

declare module "jsqr" {
  interface QrResult {
    data: string;
  }
  export default function jsQR(
    data: Uint8ClampedArray,
    width: number,
    height: number,
  ): QrResult | null;
}

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}
