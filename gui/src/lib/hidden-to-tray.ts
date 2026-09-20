// ウィンドウをトレイへ格納するとき(×ボタン。Rust側が知らせる)の、開いている画面の扱いを決める純関数。
//
// 格納しても、WebViewは動き続ける。そのまま長時間置かれると、パスフレーズ・復号済みの内容・.envの値を持つ
// 画面が、格納している間、画面と状態に残り続けるため、それらを持つ画面は閉じる(JSの文字列はメモリ上で
// 消去できないため、閉じて消えるのは参照だけである)。
//   keep: そのままにする(パスフレーズなどを持たない画面)
//   close: 閉じる(利用者が取り消したのと同じ扱い)
//   conceal: 閉じずに、パスフレーズの表示を伏せ字へ戻す

import type { ExportPhase } from "./export-dialog-state";

export type HiddenToTrayAction = "keep" | "close" | "conceal";

// パスフレーズ・復号済みの内容・.envの値を持たない画面の種類(メイン画面・プロファイル管理画面のDialogState.kind)。
// メイン画面のファイル取り込みの確認は、編集欄が持つ本文と同じ内容しか持たないため、そのままにする。
// ここに無い種類(新しく足した画面を含む)は、閉じる。秘匿情報を持つ画面が、格納したまま残らないようにするため。
const KEPT_KINDS: ReadonlySet<string> = new Set([
  "none",
  "newProfile",
  "templateSelect",
  "profileNameFromTemplate",
  "tagManagement",
  "fileImportChoice",
  "overwriteConfirm",
  "matchCountConfirm",
]);

export function actionOnHiddenToTray(kind: string, exportPhase?: ExportPhase): HiddenToTrayAction {
  if (kind === "export") {
    // 書き込み中に閉じると、パスフレーズを失ったまま、ファイルだけが書き出される。書き出し済みの画面を閉じると、
    // 書き出したファイルを復号するためのパスフレーズを、二度と表示できない。どちらも閉じない。
    if (exportPhase === "writing") return "keep";
    if (exportPhase === "exported") return "conceal";
    return "close";
  }
  return KEPT_KINDS.has(kind) ? "keep" : "close";
}
