import { invoke } from "@tauri-apps/api/core";

export interface ConnectionPreset {
  id: string;
  name: string;
  baseUrl: string;
  apiKey: string;
}

export const connectionPresetsApi = {
  list: (): Promise<ConnectionPreset[]> => invoke("list_connection_presets"),
  save: (preset: ConnectionPreset): Promise<void> =>
    invoke("save_connection_preset", { preset }),
  remove: (id: string): Promise<void> =>
    invoke("delete_connection_preset", { id }),
};
