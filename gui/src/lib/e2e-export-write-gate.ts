// エクスポートの書き込みを、E2Eテストが指定するまで保留する薄い口。
//
// 書き込み中(書き込みを始めてから、終えるまで)の画面は、その間に、WebDriverから操作して確かめたい。しかし、
// 書き込みのIPCの応答は、テスト側から保留できない。そこでE2Eビルド(VITE_E2E_TESTING)に限り、書き込みを
// 始める前に、window.__e2eExportWriteGateへ指定された待ちが終わるまで待つ(指定が無ければ、待たない)。
// 本番ビルドではVITE_E2E_TESTINGが未定義になり、この分岐は成果物から取り除かれる
// (dist内に"__e2e"が残らないことをビルド後に確認する)。

declare global {
  interface Window {
    __e2eExportWriteGate?: Promise<void>;
    // 書き込みが、この口へ来た回数。書き込みを始めてはならない場面で、始めていないことを、書き込みの所要時間
    // (負荷で伸びる)に頼らずに確かめるために使う。
    __e2eExportWriteAttempts?: number;
  }
}

// 環境変数は文字列のため、"false"などを誤って真と扱わないよう、"true"との一致で判定する。
export async function waitForE2eExportWriteGate(): Promise<void> {
  if (import.meta.env.VITE_E2E_TESTING === "true") {
    window.__e2eExportWriteAttempts = (window.__e2eExportWriteAttempts ?? 0) + 1;
    await window.__e2eExportWriteGate;
  }
}
