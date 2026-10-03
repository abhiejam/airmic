import { invoke } from "@tauri-apps/api/core";

// Types mirror docs/ipc.md §2. Keep them in step with that file.

export type Status = {
  state: "idle" | "streaming" | "muted";
  phone: { id: string; name: string; addr: string } | null;
  stats: { loss_pct: number; jitter_ms: number; latency_ms: number } | null;
  audio_flowing: boolean;
  is_default_source: boolean;
  version: string;
};

export type Level = { rms: number; peak: number };

export type PairingCode = {
  code: string;
  expires_at: number;
  qr: string;
};

export type PairedDevice = {
  phone_id: string;
  name: string;
  paired_at: number;
  last_seen: number | null;
};

export type Settings = {
  set_default_source: boolean;
  control_port: number;
  audio_port: number;
  transcription: { enabled: boolean; model: "base.en" | "small.en" };
};

export type SettingsPatch = Partial<Omit<Settings, "transcription">> & {
  transcription?: Partial<Settings["transcription"]>;
};

/** The daemon's error (docs/ipc.md §5), or code -32000 when the daemon could not be reached. */
export type IpcError = { code: number; message: string };

function call<T>(method: string, params?: object): Promise<T> {
  return invoke<T>("ipc_call", { method, params });
}

export const daemon = {
  status: () => call<Status>("status"),
  pairingCode: (regenerate = false) => call<PairingCode>("pairing_code", { regenerate }),
  pairedDevices: () => call<PairedDevice[]>("paired_devices"),
  forgetDevice: (phoneId: string) => call<null>("forget_device", { phone_id: phoneId }),
  getSettings: () => call<Settings>("get_settings"),
  setSettings: (patch: SettingsPatch) => call<Settings>("set_settings", patch),
  makeDefault: () => call<null>("make_default"),
};
