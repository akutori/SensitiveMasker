// インポートの確認画面(「インポート実行」「キャンセル」)の操作の流れ。
// プロファイル管理画面とメイン画面が、同じ流れを使うために共有する。
//
// 「インポート実行」は、確定(commit)を始めた後で、画面を閉じる操作(onOpenChange(false))も呼ぶ
// (この呼び出し順は、Radixのボタンの実装が保証する)。その閉じる操作から、保留中の内容の破棄
// (discardPending)を続けて発行すると、2つの操作の実行順が保証されず、破棄が先に走ると、確定が
// 「確認待ちのインポートがありません」で失敗する。確定は、Rust側に届いた時点で、成否に関わらず
// 保留中の内容を消費するため、確定を始めた場合は、破棄を発行しない。

export interface ImportConfirmDeps {
  // 確認画面がいま開いているか(閉じる途中に届いた2回目の押下を、確定にしないため)。
  isOpen: () => boolean;
  // 確定する。失敗の通知は、この関数の側で行う。
  commit: () => Promise<void>;
  // 保留中の(復号済みの)内容を、Rust側から破棄する。
  discardPending: () => void;
  close: () => void;
  // 確定の完了を待つ間に、別の画面が開かれていた場合は閉じない(確認画面のままの場合だけ閉じる)。
  closeIfStillOpen: () => void;
}

// startedは、確定を始めてから終えるまでの間だけtrueになる。再描画をまたいで保つため、呼び出し側が
// 持つ(refを渡す)。
export function createImportConfirmHandlers(deps: ImportConfirmDeps, started: { current: boolean }) {
  return {
    onOpenChange(open: boolean) {
      if (open) return;
      deps.close();
      if (started.current) return;
      // キャンセルなど、確定しない閉じ方では、ここで明示的に破棄しない限り、復号済みの平文が
      // 残り続ける。
      deps.discardPending();
    },
    // 画面が破棄される(離れる)とき。復号済みの内容を確認する人が居なくなるため、破棄する。確認画面が開いて
    // いるかは見ない: 復号の結果が届いてから確認画面が描画されるまでの間に離れる場合も、保留を残さないため
    // (保留が無ければ、破棄は何も起こさない)。確定を始めていれば、その確定が保留を使うため、破棄しない。
    onLeave() {
      if (started.current) return;
      deps.discardPending();
    },
    async onConfirm() {
      if (started.current || !deps.isOpen()) return;
      started.current = true;
      try {
        await deps.commit();
      } catch {
        // 失敗の通知は、commitの側が行う(ここで握りつぶさないと、このPromiseがunhandledになる)。
      } finally {
        started.current = false;
        deps.closeIfStillOpen();
      }
    },
  };
}
