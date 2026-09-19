// ネイティブのファイルダイアログ(plugin:dialog)の呼び出しを1か所に集約する薄いラッパー。
//
// ネイティブダイアログはWebDriverから操作できず、E2Eではテスト側からinvokeを差し替える
// こともできない。そのためE2Eビルド(VITE_E2E_TESTING)に限り、テストが
// window.__e2eFileDialogPathsへ指定したパスを、ダイアログの代わりに返す。
// 本番ビルドではVITE_E2E_TESTINGが未定義になり、この分岐は成果物から取り除かれる
// (dist内に"__e2eFileDialogPaths"が残らないことをビルド後に確認する)。
import {
  open,
  save,
  type OpenDialogOptions,
  type OpenDialogReturn,
  type SaveDialogOptions,
} from "@tauri-apps/plugin-dialog";

interface E2eFileDialogPaths {
  save?: string;
  open?: string;
}

declare global {
  interface Window {
    __e2eFileDialogPaths?: E2eFileDialogPaths;
  }
}

// テストが指定していない場合はundefinedを返し、本物のダイアログへ進ませる。
function e2ePath(kind: keyof E2eFileDialogPaths): string | undefined {
  if (!import.meta.env.VITE_E2E_TESTING) return undefined;
  return window.__e2eFileDialogPaths?.[kind];
}

export async function saveFileDialog(options?: SaveDialogOptions): Promise<string | null> {
  return e2ePath("save") ?? save(options);
}

// 差し替え時に返すのは単一のパスのみ(このアプリのopen呼び出しはmultiple: falseのみ)。
export async function openFileDialog<T extends OpenDialogOptions>(
  options?: T
): Promise<OpenDialogReturn<T>> {
  const path = e2ePath("open");
  if (path !== undefined) return path as unknown as OpenDialogReturn<T>;
  return open(options);
}
