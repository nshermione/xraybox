<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import jsQR from "jsqr";
import { t } from "../state/locale";
import { isDesktop } from "../platform";
import Modal from "./Modal.vue";

const emit = defineEmits<{
  url: [value: string];
}>();

const error = ref<string | null>(null);
const cameraOpen = ref(false);
const scanning = ref(false);
const video = ref<HTMLVideoElement | null>(null);
const fileInput = ref<HTMLInputElement | null>(null);
let stream: MediaStream | null = null;
let frame = 0;

const modalVisible = computed(() => (isDesktop ? error.value !== null : cameraOpen.value));

onUnmounted(() => stopCamera());

function stopCamera(): void {
  scanning.value = false;
  if (frame) cancelAnimationFrame(frame);
  frame = 0;
  stream?.getTracks().forEach((track) => track.stop());
  stream = null;
}

function openCamera(): void {
  if (isDesktop) return;
  error.value = null;
  cameraOpen.value = true;
  void startCamera();
}

async function startCamera(): Promise<void> {
  error.value = null;
  try {
    const next = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: { ideal: "environment" } },
      audio: false,
    });
    if (!cameraOpen.value) {
      next.getTracks().forEach((track) => track.stop());
      return;
    }
    stream = next;
    scanning.value = true;
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    if (video.value) {
      video.value.srcObject = stream;
      await video.value.play();
      scanFrame();
    }
  } catch {
    stopCamera();
    error.value = t("subscription.noCamera");
  }
}

function scanFrame(): void {
  if (!scanning.value || !video.value) return;
  const el = video.value;
  if (el.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) {
    const canvas = document.createElement("canvas");
    canvas.width = el.videoWidth;
    canvas.height = el.videoHeight;
    const ctx = canvas.getContext("2d");
    if (ctx && canvas.width > 0) {
      ctx.drawImage(el, 0, 0);
      const image = ctx.getImageData(0, 0, canvas.width, canvas.height);
      const result = jsQR(image.data, image.width, image.height);
      if (result?.data) {
        acceptPayload(result.data);
        return;
      }
    }
  }
  frame = requestAnimationFrame(scanFrame);
}

function pickImage(): void {
  fileInput.value?.click();
}

async function onFile(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = "";
  if (!file) return;
  error.value = null;
  try {
    const bitmap = await createImageBitmap(file);
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("canvas");
    ctx.drawImage(bitmap, 0, 0);
    const image = ctx.getImageData(0, 0, canvas.width, canvas.height);
    const result = jsQR(image.data, image.width, image.height);
    if (!result?.data) {
      error.value = t("subscription.invalidQr");
      return;
    }
    acceptPayload(result.data);
  } catch {
    error.value = t("subscription.invalidQr");
  }
}

function acceptPayload(raw: string): void {
  const value = raw.trim();
  if (!/^https?:\/\//i.test(value)) {
    error.value = t("subscription.invalidQr");
    return;
  }
  stopCamera();
  cameraOpen.value = false;
  error.value = null;
  emit("url", value);
}

function close(): void {
  stopCamera();
  cameraOpen.value = false;
  error.value = null;
}

defineExpose({ pickImage, openCamera });
</script>

<template>
  <input ref="fileInput" type="file" accept="image/*" hidden @change="onFile" />
  <Modal v-if="modalVisible" :title="t('home.scan')" @close="close">
    <div v-if="!isDesktop" class="preview">
      <video ref="video" muted playsinline autoplay />
      <button type="button" class="image-btn" :aria-label="t('home.pickImage')" @click="pickImage">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="currentColor"
            d="M21 19V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2zM8.5 13.5 11 16.51 14.5 12 19 18H5z"
          />
        </svg>
      </button>
    </div>
    <div v-else class="retry">
      <button type="button" class="image-btn" :aria-label="t('home.pickImage')" @click="pickImage">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="currentColor"
            d="M21 19V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2zM8.5 13.5 11 16.51 14.5 12 19 18H5z"
          />
        </svg>
      </button>
    </div>
    <p v-if="error" class="error">{{ error }}</p>
  </Modal>
</template>

<style scoped>
.preview {
  position: relative;
}

.retry {
  display: flex;
  justify-content: flex-end;
}

video {
  display: block;
  width: 100%;
  min-height: 220px;
  object-fit: cover;
  border-radius: var(--radius-md);
  background: #111;
}

.image-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  padding: 0;
  border: 0;
  border-radius: 10px;
  background: rgb(0 0 0 / 55%);
  color: #fff;
  cursor: pointer;
}

.preview .image-btn {
  position: absolute;
  right: 10px;
  bottom: 10px;
}

.retry .image-btn {
  background: var(--color-press);
  color: var(--color-accent);
}

.image-btn svg {
  width: 22px;
  height: 22px;
}

.error {
  margin: 10px 0 0;
  color: var(--color-danger);
  font-size: 13px;
}
</style>
