// 鍵ファイル(エクスポートしたファイルを復号するための、.smxkeyのファイル)の、拡張子の判定と、ドラッグ&ドロップで
// 受け取ったパスから、鍵ファイルを選ぶ純関数。鍵の中身は、画面(JavaScript)へ渡さない。Rustが、生成・保存・読み込みを行う。

export const KEY_FILE_EXTENSION = "smxkey";

export const KEY_FILE_FILTERS = [{ name: "SensitiveMaskerの鍵ファイル", extensions: [KEY_FILE_EXTENSION] }];

// 鍵ファイル方式のエクスポートの、既定のファイル名(拡張子の前)。日時を含める: 固定の名前だと、2回目のエクスポートで、
// 前のエクスポートの鍵ファイルを、置き換えてしまいやすい(置き換えると、前のエクスポートしたファイルを、別の名前で
// 残していても、二度と復号できなくなる)。プロファイル名は、ファイル名(最近使ったファイルの履歴)に、平文の
// メタデータとして残るため、使わない。
export function keyFileExportBaseName(all: boolean, now: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  const date = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `${all ? "sensitivemasker_all" : "sensitivemasker_export"}_${date}-${time}`;
}

export function keyFileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

// 名前(拡張子の前)が空でない、.smxkeyのファイルか。大文字小文字は区別しない。
export function hasKeyFileExtension(path: string): boolean {
  const name = keyFileNameOf(path);
  const dot = name.lastIndexOf(".");
  return dot > 0 && name.slice(dot + 1).toLowerCase() === KEY_FILE_EXTENSION;
}

export type KeyFilePick = { kind: "picked"; path: string } | { kind: "rejected"; message: string };

// ウィンドウへドロップされたファイルのパスから、鍵ファイルを1つ選ぶ。ちょうど1つで、拡張子が.smxkeyのときだけ選ぶ
// (複数のファイルから、どれが鍵ファイルかを、推測しない)。
export function pickKeyFileFromDroppedPaths(paths: string[]): KeyFilePick {
  if (paths.length === 0) return { kind: "rejected", message: "ドロップされたファイルが見つかりません" };
  if (paths.length > 1) return { kind: "rejected", message: "鍵ファイルは、1つだけドロップしてください" };
  const [path] = paths;
  if (!hasKeyFileExtension(path)) {
    return { kind: "rejected", message: "拡張子が.smxkeyのファイルをドロップしてください" };
  }
  return { kind: "picked", path };
}
