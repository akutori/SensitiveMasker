// エクスポート画面の進行状況(編集中→実行中→成功後)と、その間に許される操作を表す純関数群。
//
// 書き出したファイルを復号できるパスフレーズは、実行した時点のものだけである。
// 実行中・成功後に再生成や再実行を許すと、画面のパスフレーズとファイルが食い違うため、
// 再生成と実行は編集中に限る。パスフレーズは画面の状態(この型)の一部として持つ。
// 画面を閉じる・別の画面へ置き換えることが、そのままパスフレーズを手放すことになるようにするため
// (JSの文字列はメモリ上で消去できないため、消えるのは参照だけである)。
// 閉じて開き直した後に届く古い実行の完了通知など、非同期の競合をReactの外側で
// 検証できるよう、状態遷移だけをここへ切り出している。

export type ExportPhase = "editing" | "exporting" | "exported";

export interface ExportDialogState {
  phase: ExportPhase;
  // 画面を開いた回の番号(開くたびに変わる)。閉じて開き直した後に、古い実行の完了通知が
  // 新しい画面へ反映されないようにするために使う。
  sessionId: number;
  passphrase: string;
}

export function openExportDialog(sessionId: number, passphrase: string): ExportDialogState {
  return { phase: "editing", sessionId, passphrase };
}

// 書き出しが始まった後は、この画面のパスフレーズが、書き出したファイルを開ける唯一の手がかりになる。
// 誤操作(Escape・背景クリック・履歴の移動など)で、画面ごと失わせてはならない局面かどうか。
// 編集中は、まだ何も書き出していないので、失っても失うものがない。
export function isPassphraseAtRisk(phase: ExportPhase): boolean {
  return phase !== "editing";
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

// 実行を押した時点の画面(開いた回の番号とパスフレーズ)と、今の状態が一致するときだけ実行中にする。
// 食い違ったまま書き出すと、画面に出ているものと違うパスフレーズでファイルができてしまう。
export function beginExport(
  state: ExportDialogState,
  sessionId: number,
  passphrase: string
): ExportDialogState {
  return state.phase === "editing" &&
    state.sessionId === sessionId &&
    state.passphrase === passphrase
    ? { ...state, phase: "exporting" }
    : state;
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
