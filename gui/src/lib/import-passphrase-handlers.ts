// インポートのパスフレーズ入力画面の「OK」(復号して、内容の確認画面へ進む)の流れ。
// プロファイル管理画面とメイン画面が、同じ流れを使うために共有する。
//
// 用語: 開いた回の番号(session)は、パスフレーズ入力画面を開くたびに変わる番号(同じファイルで開き直しても、
// 別の画面として区別する)。保留の識別子(pendingId)は、Rust側に保留された復号済みの内容の識別子(復号のたびに払い出される)。
//
// 復号(scrypt)は数百ミリ秒〜数秒かかり、その間もこの画面は操作できる。
// - 復号している間に、もう一度OKが押されると、同じ復号が並行して走り、負荷が無駄に増える。
//   復号している間は、再入を受け付けない。
// - 復号している間に画面が閉じられる(同じファイルで開き直される場合も含む)と、復号が終わった時点で、
//   復号済みの内容がRust側に保留される(確認画面へ進む画面が、もう無いのに)。その内容が残り続けない
//   よう、OKを押した時の画面(その開いた回の番号)が開いたままでなければ、届いた結果の保留を、その保留の識別子を
//   指定して破棄する(他の画面・他の復号の保留には触れない)。破棄が終わるまでは、待ちを解かない
//   (この画面が始めた復号の後始末が終わってから、次の復号を受け付ける)。
// - 復号の結果が届き、確認画面へ進むときは、その保留の識別子を、この画面が所有するものとして、確認画面を
//   出すより前に記録する。確認画面の描画を待つと、その前に画面を離れたときに、保留を破棄できない。
// - 記録する時点で、既に所有している保留(確認画面の描画より前に画面が閉じられて、確認されないまま残ったもの)が
//   あれば、新しい保留を所有として記録したうえで、その(前の)保留の識別子の破棄を発行する。上書きするだけだと、
//   その保留は誰にも破棄されなくなる。この破棄の完了は待たない(待つ間に画面が閉じられうるため、記録と確認画面へ
//   進む操作は、同期的に続ける)。

// 復号の結果。pendingIdは、復号済みの内容をRust側に保留した、保留の識別子。
export interface PendingPreview {
  pendingId: number;
}

export interface ImportPassphraseDeps<Preview extends PendingPreview> {
  // OKを押した時点の、入力画面の対象。開いていなければ(閉じる途中に届いた押下)null。
  // sessionは、画面を開いた回の番号(開くたびに変わる。同じファイルを開き直しても、別の画面として区別する)。
  target: () => { session: number; sourcePath: string; passphrase: string } | null;
  preview: (sourcePath: string, passphrase: string) => Promise<Preview>;
  // 復号が終わった時点で、OKを押した時の画面(その開いた回の番号がsession)が、まだ開いているか(最新の状態で判定する)。
  isStillOpen: (session: number) => boolean;
  showConfirm: (preview: Preview) => void;
  showError: (error: unknown) => void;
  // 保留の識別子で指定した保留(Rust側の、復号済みの内容)だけを破棄する。
  discardPending: (pendingId: number) => Promise<void>;
  // 復号を始める・終えるたびに呼ぶ(OKなどの無効化の表示に使う)。
  onBusyChange: (busy: boolean) => void;
}

// decryptingは、復号を始めてから終えるまでの間だけtrueになる。ownedPendingIdは、この画面が所有する保留の
// 識別子(確認画面へ進んだ結果のもの。無ければnull)で、確認画面の操作(import-confirm-handlers.ts)が使う。
// どちらも再描画をまたいで保つため、呼び出し側が持つ(refを渡す)。
export function createImportPassphraseHandlers<Preview extends PendingPreview>(
  deps: ImportPassphraseDeps<Preview>,
  decrypting: { current: boolean },
  ownedPendingId: { current: number | null }
) {
  return {
    async onConfirm() {
      if (decrypting.current) return;
      const target = deps.target();
      if (!target) return;
      decrypting.current = true;
      deps.onBusyChange(true);
      try {
        const preview = await deps.preview(target.sourcePath, target.passphrase);
        if (!deps.isStillOpen(target.session)) {
          await deps.discardPending(preview.pendingId);
          return;
        }
        const previousPendingId = ownedPendingId.current;
        // この結果の保留を所有する記録は、前の保留の破棄を発行するより先に行う(破棄の発行の失敗が、新しい保留の
        // 所有の記録に影響しないようにするため。同期的な失敗は、下のtry/catchも握りつぶす)。
        ownedPendingId.current = preview.pendingId;
        if (previousPendingId !== null) {
          // 破棄の失敗(同期的な失敗を含む)は、前の保留の後始末の失敗であり、この結果の失敗ではない。
          try {
            void Promise.resolve(deps.discardPending(previousPendingId)).catch(() => {});
          } catch {
            // 同期的に失敗した場合も、この結果は、確認画面へ進める。
          }
        }
        deps.showConfirm(preview);
      } catch (error) {
        // 閉じられた後の失敗(破棄の失敗を含む)は、見る人がいないため、表示しない。
        if (deps.isStillOpen(target.session)) deps.showError(error);
      } finally {
        decrypting.current = false;
        deps.onBusyChange(false);
      }
    },
  };
}
