import { invoke } from "@tauri-apps/api/core";

export interface EnvCandidate {
  key: string;
  value: string;
  includedByDefault: boolean;
}

interface EnvCandidateResponse {
  key: string;
  value: string;
  included_by_default: boolean;
}

export async function previewEnvImport(content: string): Promise<EnvCandidate[]> {
  const response = await invoke<EnvCandidateResponse[]>("preview_env_import", { content });
  return response.map((c) => ({ key: c.key, value: c.value, includedByDefault: c.included_by_default }));
}
