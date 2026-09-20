// 右クリック(・メニューキー・Shift+F10)のメニュー(contextmenuイベントの既定の動作)を、出さないようにする。
//
// WebView2の既定のメニューは、ページ全体を操作する項目(戻る・保存・印刷など)を含みうる。書き出し中・書き出し済みの
// エクスポート画面は、画面ごと消えると、パスフレーズを失うため、その間だけ使う(履歴の移動を止めるのと同じ理由)。
// ページ側でイベントを止めるので、WebView2以外(WKWebView・WebKitGTK)でも効く。
//
// 全てのcontextmenuを、キャプチャの段階で止める。画面の中の要素が、イベントの伝播を止めても、影響されないため。
// 返り値は、止めるのをやめる関数。

export function suppressContextMenu(target: EventTarget): () => void {
  const suppress = (event: Event) => event.preventDefault();
  // キャプチャは、真偽値でなく、オプションで指定する(解除の側で、真偽値を、キャプチャとして扱わない実装があるため)。
  target.addEventListener("contextmenu", suppress, { capture: true });
  return () => target.removeEventListener("contextmenu", suppress, { capture: true });
}
