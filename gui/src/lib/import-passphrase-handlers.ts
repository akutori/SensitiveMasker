// インポートのパスフレーズ入力画面の「OK」(復号して、内容の確認画面へ進む)の流れ。
// プロファイル管理画面とメイン画面が、同じ流れを使うために共有する。
//
// 復号(scrypt)は数百ミリ秒〜数秒かかり、その間もこの画面は操作できる。
// - 復号している間に、もう一度OKが押されると、同じ復号が並行して走り、負荷が無駄に増える。
//   復号している間は、再入を受け付けない。
// - 復号している間に画面が閉じられると、復号が終わった時点で、復号済みの内容がRust側に保留される
//   (確認画面へ進む画面が、もう無いのに)。その内容が残り続けないよう、閉じられていたら破棄する。

export interface ImportPassphraseDeps<Preview> {
  // OKを押した時点の、入力画面の対象。開いていなければ(閉じる途中に届いた押下)null。
  target: () => { sourcePath: string; passphrase: string } | null;
  preview: (sourcePath: string, passphrase: string) => Promise<Preview>;
  // 復号が終わった時点で、同じファイルの入力画面が、まだ開いているか(最新の状態で判定する)。
  isStillOpen: (sourcePath: string) => boolean;
  showConfirm: (preview: Preview) => void;
  showError: (error: unknown) => void;
  // Rust側に保留された、復号済みの内容を破棄する。
  discardPending: () => void;
  // 復号を始める・終えるたびに呼ぶ(OKなどの無効化の表示に使う)。
  onBusyChange: (busy: boolean) => void;
}

// decryptingは、復号を始めてから終えるまでの間だけtrueになる。再描画をまたいで保つため、呼び出し側が
// 持つ(refを渡す)。
export function createImportPassphraseHandlers<Preview>(
  deps: ImportPassphraseDeps<Preview>,
  decrypting: { current: boolean }
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
        if (!deps.isStillOpen(target.sourcePath)) {
          deps.discardPending();
          return;
        }
        deps.showConfirm(preview);
      } catch (error) {
        // 閉じられた後の失敗は、見る人がいないため、表示しない。
        if (deps.isStillOpen(target.sourcePath)) deps.showError(error);
      } finally {
        decrypting.current = false;
        deps.onBusyChange(false);
      }
    },
  };
}
