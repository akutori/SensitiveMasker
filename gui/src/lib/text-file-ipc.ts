import { invoke } from "@tauri-apps/api/core";

export interface ReadTextFileResult {
  text: string;
  hadInvalidUtf8: boolean;
}

interface ReadTextFileResponse {
  text: string;
  had_invalid_utf8: boolean;
}

export async function readTextFile(path: string): Promise<ReadTextFileResult> {
  const response = await invoke<ReadTextFileResponse>("read_text_file", { path });
  return { text: response.text, hadInvalidUtf8: response.had_invalid_utf8 };
}

export async function writeTextFile(path: string, content: string): Promise<void> {
  await invoke("write_text_file", { path, content });
}
