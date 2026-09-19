// エクスポート画面の進行状況(編集中→実行中→成功後)と、その間に許される操作を表す純関数群。
//
// 書き出したファイルを復号できるパスフレーズは、実行した時点のものだけである。
// 実行中・成功後に再生成や再実行を許すと、画面のパスフレーズとファイルが食い違うため、
// 再生成と実行は編集中に限る。パスフレーズは画面の状態(この型)の一部として持つ。
// 画面を閉じる・別の画面へ置き換えることが、そのままパスフレーズの破棄になるようにするため。
// 閉じて開き直した後に届く古い実行の完了通知など、非同期の競合をReactの外側で
// 検証できるよう、状態遷移だけをここへ切り出している。

export type ExportPhase = "editing" | "exporting" | "exported";

export interface ExportDialogState {
  phase: ExportPhase;
  // 画面を開くたびに変わる識別子。閉じて開き直した後に、古い実行の完了通知が
  // 新しい画面へ反映されないようにするために使う。
  sessionId: number;
  passphrase: string;
}

export function openExportDialog(sessionId: number, passphrase: string): ExportDialogState {
  return { phase: "editing", sessionId, passphrase };
}

export function canRegenerate(state: ExportDialogState): boolean {
  return state.phase === "editing";
}

export function canStartExport(state: ExportDialogState): boolean {
  return state.phase === "editing";
}

export function regeneratePassphrase(
  state: ExportDialogState,
  passphrase: string
): ExportDialogState {
  return canRegenerate(state) ? { ...state, passphrase } : state;
}

export function beginExport(state: ExportDialogState): ExportDialogState {
  return state.phase === "editing" ? { ...state, phase: "exporting" } : state;
}

export function completeExport(state: ExportDialogState, sessionId: number): ExportDialogState {
  return state.phase === "exporting" && state.sessionId === sessionId
    ? { ...state, phase: "exported" }
    : state;
}

export function abortExport(state: ExportDialogState, sessionId: number): ExportDialogState {
  return state.phase === "exporting" && state.sessionId === sessionId
    ? { ...state, phase: "editing" }
    : state;
}
