// 応答待ちの非同期処理の件数を数え、1件以上ある間は「待ち」であることを知らせる小さな部品。
//
// クリップボードのコピー/クリアの応答待ちの間は、コピーと再生成を受け付けない。応答待ちの間に
// 他方を押すと世代カウンタが進み、後から解決した側が世代不一致で早期returnして、自動クリアの
// 設置が行われなくなるため。件数で数えるのは、自動クリアのタイマー発火によるクリアが他の操作と
// 重なっても、最後の1件が終わるまで受け付けないままにするため。

export interface OperationCounter {
  // 再描画を待たずに、同期的に判定できる(ボタンの無効化が反映される前に届いた操作を防ぐため)。
  isBusy: () => boolean;
  // 成功・失敗のどちらでも必ず件数を戻す(戻し忘れると、操作が無効のまま戻らなくなるため)。
  // 処理の結果(値・失敗)は、そのまま呼び出し元へ伝える。
  track: <T>(operation: Promise<T>) => Promise<T>;
}

// onChangeには、件数が変わるたびに、待ちかどうかを渡す(画面の無効化の表示に使う)。
export function createOperationCounter(onChange: (busy: boolean) => void): OperationCounter {
  let count = 0;
  return {
    isBusy: () => count > 0,
    track: (operation) => {
      count += 1;
      onChange(true);
      return operation.finally(() => {
        count = Math.max(0, count - 1);
        onChange(count > 0);
      });
    },
  };
}
