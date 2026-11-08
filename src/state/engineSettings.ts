import { reactive } from "vue";
import { getEngineSettings, saveEngineSettings, type EngineSettings } from "../engine/tauri";

export const engineSettings = reactive<EngineSettings>({
  autoConnect: false,
  startOnBoot: false,
  killSwitch: false,
  smartConnect: false,
  logsEnabled: true,
});

export async function hydrateEngineSettings(): Promise<void> {
  try {
    applySaved(await getEngineSettings());
  } catch {
    // Vite-only preview has no Tauri runtime.
  }
}

export async function setAutoConnect(enabled: boolean): Promise<void> {
  await persist({ autoConnect: enabled });
}

export async function setStartOnBoot(enabled: boolean): Promise<void> {
  await persist({ startOnBoot: enabled });
}

export async function setKillSwitch(enabled: boolean): Promise<void> {
  await persist({ killSwitch: enabled });
}

export async function setSmartConnect(enabled: boolean): Promise<void> {
  await persist({ smartConnect: enabled });
}

export async function setLogsEnabled(enabled: boolean): Promise<void> {
  await persist({ logsEnabled: enabled });
}

function snapshot(): EngineSettings {
  return {
    autoConnect: engineSettings.autoConnect,
    startOnBoot: engineSettings.startOnBoot,
    killSwitch: engineSettings.killSwitch,
    smartConnect: engineSettings.smartConnect,
    logsEnabled: engineSettings.logsEnabled,
  };
}

function applySaved(settings: EngineSettings): void {
  engineSettings.autoConnect = Boolean(settings.autoConnect);
  engineSettings.startOnBoot = Boolean(settings.startOnBoot);
  engineSettings.killSwitch = Boolean(settings.killSwitch);
  engineSettings.smartConnect = Boolean(settings.smartConnect);
  engineSettings.logsEnabled = settings.logsEnabled !== false;
}

async function persist(patch: Partial<EngineSettings>): Promise<void> {
  const previous = snapshot();
  applySaved({ ...previous, ...patch });
  try {
    applySaved(await saveEngineSettings({ ...previous, ...patch }));
  } catch {
    applySaved(previous);
  }
}
