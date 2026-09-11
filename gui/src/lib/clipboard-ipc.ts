import { invoke } from "@tauri-apps/api/core";

// Rust側のClipboardClearOutcomeに対応する型。SkippedUnableToVerifyのみ、ユーザーに
// 手動でのクリアを促す必要がある(Clearedは成功、SkippedContentChangedは既に別の
// 内容へ上書き済みで安全なため、どちらも追加の警告は不要)。
export type ClipboardClearOutcome =
  | { outcome: "cleared" }
  | { outcome: "skipped_content_changed" }
  | { outcome: "skipped_unable_to_verify" };

// navigator.clipboard.writeText()と異なりウィンドウのフォーカスに依存しない。
export async function writeClipboardText(text: string): Promise<void> {
  await invoke("write_clipboard_text", { text });
}

// 現在のクリップボードの内容がexpectedのままであればクリアする。読み取り・比較・
// クリアをRust側で完結させ、navigator.clipboard.readText()が要求するウィンドウ
// フォーカス・clipboard-read権限に依存しないようにする。
export async function clearClipboardIfMatches(expected: string): Promise<ClipboardClearOutcome> {
  return invoke<ClipboardClearOutcome>("clear_clipboard_if_matches", { expected });
}
