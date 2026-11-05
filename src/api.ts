import { invoke } from "@tauri-apps/api/core";

export const IS_WINDOWS = navigator.userAgent.includes("Windows");

export type EngineKind = "ClaudeCli" | "ApiKey";
export interface Settings {
  input_device: string | null;
  read_dirs: string[];
  notes_path: string;
  hotkey: string;
  diagnostic: boolean;
  engine: EngineKind;
  onboarded: boolean;
}
export interface InputDevice {
  name: string;
  bluetooth: boolean;
}

export const ensureModels = () => invoke<void>("ensure_models");
export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (s: Settings) => invoke<void>("save_settings", { s });
export const listInputs = () => invoke<InputDevice[]>("list_inputs");
export const inputLevel = (device: string) => invoke<number>("input_level", { device });
export const stopInputLevel = () => invoke<void>("stop_input_level");
export type MicPermission = "authorized" | "denied" | "restricted" | "undetermined";
export const micPermission = () => invoke<MicPermission>("mic_permission");
export const checkClaude = () => invoke<string>("check_claude");
export const checkVoice = () => invoke<boolean>("check_voice");
export const testWake = () => invoke<boolean>("test_wake");

/** RMS 轉 dB；0 視為 -120 dB。 */
export const toDb = (rms: number) => (rms > 0 ? 20 * Math.log10(rms) : -120);

/** 建立元素的小工具。 */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> & { class?: string } = {},
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  const { class: cls, ...rest } = props;
  if (cls) el.className = cls;
  Object.assign(el, rest);
  el.append(...children);
  return el;
}
