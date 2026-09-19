// インポートで、Rust側に保留された、復号済みの内容の識別子を、E2Eテストが読める場所へ残す薄い口。
//
// 保留の確定・破棄は、識別子を指定して行う。E2Eは、画面の外(WebDriver)から、確定を直接呼んで、保留が残って
// いないことを確かめるため、アプリが受け取った識別子を知る必要がある。そのためE2Eビルド(VITE_E2E_TESTING)に
// 限り、受け取った識別子を、受け取った順に、window.__e2ePendingImportIdsへ追記する(直近のものが末尾)。
// 本番ビルドではVITE_E2E_TESTINGが未定義になり、この分岐は成果物から取り除かれる
// (dist内に"__e2e"が残らないことをビルド後に確認する)。

declare global {
  interface Window {
    __e2ePendingImportIds?: number[];
  }
}

// 環境変数は文字列のため、"false"などを誤って真と扱わないよう、"true"との一致で判定する。
export function recordPendingImportIdForE2e(pendingId: number): void {
  if (import.meta.env.VITE_E2E_TESTING === "true") {
    (window.__e2ePendingImportIds ??= []).push(pendingId);
  }
}
