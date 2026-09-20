// ネイティブのファイルダイアログ(plugin:dialog)の呼び出しを1か所に集約する薄いラッパー。
//
// ネイティブダイアログはWebDriverから操作できず、E2Eではテスト側からinvokeを差し替える
// こともできない。そのためE2Eビルド(VITE_E2E_TESTING)に限り、テストが
// window.__e2eFileDialogPathsへ指定した値を、ダイアログの代わりに返す。
// 保存・開くのどちらにもPromiseを指定できる。テスト側が決着させるまで選択を保留したり、
// 取り消し(null)・失敗(reject)を再現したりして、選択を待つ間の画面や、取り消し・失敗の後の
// 画面を検証するために使う。
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
  save?: string | Promise<string | null>;
  open?: string | Promise<string | null>;
  // 鍵ファイル(.smxkey)の保存・選択用。エクスポートしたファイル(.smx)と、別に指定する。
  saveKey?: string | Promise<string | null>;
  openKey?: string | Promise<string | null>;
}

// data: エクスポートしたファイル(.smx)など。key: 鍵ファイル(.smxkey)。E2Eで、差し替える値を分けるための区別。
export type FileDialogPurpose = "data" | "key";

declare global {
  interface Window {
    __e2eFileDialogPaths?: E2eFileDialogPaths;
  }
}

// テストが指定していない場合はundefinedを返し、本物のダイアログへ進ませる。
// 環境変数は文字列のため、"false"などを誤って真と扱わないよう、"true"との一致で判定する。
function e2eReplacement<K extends keyof E2eFileDialogPaths>(kind: K): E2eFileDialogPaths[K] {
  if (import.meta.env.VITE_E2E_TESTING !== "true") return undefined;
  return window.__e2eFileDialogPaths?.[kind];
}

export async function saveFileDialog(
  options?: SaveDialogOptions,
  purpose: FileDialogPurpose = "data"
): Promise<string | null> {
  return e2eReplacement(purpose === "key" ? "saveKey" : "save") ?? save(options);
}

// 差し替え時に返すのは単一のパスのみ(このアプリのopen呼び出しはmultiple: falseのみ)。
export async function openFileDialog<T extends OpenDialogOptions>(
  options?: T,
  purpose: FileDialogPurpose = "data"
): Promise<OpenDialogReturn<T>> {
  const path = e2eReplacement(purpose === "key" ? "openKey" : "open");
  if (path !== undefined) return path as unknown as OpenDialogReturn<T>;
  return open(options);
}
