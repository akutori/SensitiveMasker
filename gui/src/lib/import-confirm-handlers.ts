// インポートの確認画面(「インポート実行」「キャンセル」)の操作の流れ。
// プロファイル管理画面とメイン画面が、同じ流れを使うために共有する。
//
// 確認画面の操作は、この画面が所有する保留(復号の結果として届いた、Rust側の復号済みの内容)だけを対象にし、
// その保留の識別子を指定して、確定・破棄する。他の画面が始めた復号の保留には、触れない。
//
// 「インポート実行」は、確定(commit)を始めた後で、画面を閉じる操作(onOpenChange(false))も呼ぶ
// (この呼び出し順は、Radixのボタンの実装が保証する)。その閉じる操作から、保留中の内容の破棄
// (discardPending)を続けて発行すると、2つの操作の実行順が保証されず、破棄が先に走ると、確定が
// 「確認待ちのインポートがありません」で失敗する。確定は、Rust側に届いた時点で、成否に関わらず
// 保留中の内容を消費するため、確定を始めた時点で、その保留を所有しない扱いにする。以後の閉じる操作・
// 離れる操作・2回目の押下は、所有する保留が無く、確定も破棄も発行しない。

export interface ImportConfirmDeps {
  // 確認画面がいま開いているか(閉じる途中に届いた2回目の押下を、確定にしないため)。
  isOpen: () => boolean;
  // 保留の識別子で指定した保留を確定する。失敗の通知は、この関数の側で行う。
  commit: (pendingId: number) => Promise<void>;
  // 保留の識別子で指定した保留(復号済みの内容)を、Rust側から破棄する。
  discardPending: (pendingId: number) => void;
  close: () => void;
  // 確定の完了を待つ間に、別の画面が開かれていた場合は閉じない(確認画面のままの場合だけ閉じる)。
  closeIfStillOpen: () => void;
}

// ownedPendingIdは、この画面が所有する保留の識別子(無ければnull。記録は、復号の結果が届いた時点で、
// import-passphrase-handlers.tsが行う)。再描画をまたいで保つため、呼び出し側が持つ(refを渡す)。
export function createImportConfirmHandlers(
  deps: ImportConfirmDeps,
  ownedPendingId: { current: number | null }
) {
  // 所有する保留を、所有しない扱いにして、その保留の識別子を返す(以後、この画面は、その保留を確定も破棄もしない)。
  const releaseOwnedPendingId = (): number | null => {
    const pendingId = ownedPendingId.current;
    ownedPendingId.current = null;
    return pendingId;
  };

  const discardOwnedPending = () => {
    const pendingId = releaseOwnedPendingId();
    if (pendingId !== null) deps.discardPending(pendingId);
  };

  return {
    onOpenChange(open: boolean) {
      if (open) return;
      deps.close();
      // キャンセルなど、確定しない閉じ方では、ここで明示的に破棄しない限り、復号済みの平文が
      // 残り続ける。
      discardOwnedPending();
    },
    // 画面が破棄される(離れる)とき。復号済みの内容を確認する人が居なくなるため、破棄する。確認画面が開いて
    // いるかは見ない: 復号の結果が届いてから確認画面が描画されるまでの間に離れる場合も、保留を残さないため
    // (所有する保留が無ければ、何もしない)。
    onLeave() {
      discardOwnedPending();
    },
    async onConfirm() {
      if (!deps.isOpen()) return;
      const pendingId = releaseOwnedPendingId();
      // 所有する保留が無い(既に確定を始めた・破棄した)なら、確定できる内容が無い。
      if (pendingId === null) return;
      try {
        await deps.commit(pendingId);
      } catch {
        // 失敗の通知は、commitの側が行う(ここで握りつぶさないと、このPromiseがunhandledになる)。
      } finally {
        deps.closeIfStillOpen();
      }
    },
  };
}
