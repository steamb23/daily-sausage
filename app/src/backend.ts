// Talks to the Rust side; falls back to sample data in a plain browser so the
// popup can be previewed with `npm run dev`.
import type { Config, InstallPreview, Snapshot } from "./types";
import { mockConfig, mockPreview, mockSnapshot } from "./mock";

export const inTauri = "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export async function getSnapshot(): Promise<Snapshot | null> {
  return inTauri ? call<Snapshot | null>("get_snapshot") : mockSnapshot();
}

export async function onSnapshot(cb: (s: Snapshot) => void): Promise<void> {
  if (!inTauri) return;
  const { listen } = await import("@tauri-apps/api/event");
  await listen<Snapshot>("snapshot", (e) => cb(e.payload));
}

let localConfig = mockConfig();

export async function getConfig(): Promise<Config> {
  return inTauri ? call<Config>("get_config") : { ...localConfig };
}

export async function setConfig(config: Config): Promise<Config> {
  if (!inTauri) {
    localConfig = { ...config };
    return { ...localConfig };
  }
  return call<Config>("set_config", { config });
}

export async function statuslinePreview(): Promise<InstallPreview> {
  return inTauri ? call<InstallPreview>("statusline_preview") : mockPreview();
}

export async function statuslineInstall(): Promise<string> {
  return inTauri ? call<string>("statusline_install") : "연동을 설치했어요. (미리보기)";
}

export async function statuslineUninstall(): Promise<string> {
  return inTauri ? call<string>("statusline_uninstall") : "연동을 제거했어요. (미리보기)";
}

export async function refresh(): Promise<void> {
  if (inTauri) await call("refresh");
}

export async function hidePopup(): Promise<void> {
  if (inTauri) await call("hide_popup");
}

export async function quit(): Promise<void> {
  if (inTauri) await call("quit");
}
