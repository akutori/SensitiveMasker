// monaco-editorが公式に同梱している日本語NLSバンドル(globalThisにメッセージテーブルを
// 設定する副作用インポート)。他のmonaco-editorモジュールより先に読み込む。
import "monaco-editor/nls/lang/ja.js";
import * as monaco from "monaco-editor";
import editorWorker from "monaco-editor/editor/editor.worker.js?worker";
import { loader } from "@monaco-editor/react";

self.MonacoEnvironment = {
  getWorker() {
    return new editorWorker();
  },
};

loader.config({ monaco });

// Monaco標準のホバーツールチップ(ツールバーアイコンの説明、find widgetの
// 閉じるボタンの「Close (Escape)」等)が狭い幅ではボタンに重なりクリック/
// キー操作を妨げるため抑制する。plaintext専用でエディタ内容へのホバー情報は
// 使わないため、無効化して問題ない。HoverServiceはhoverの描画先を`.monaco-editor`
// の子孫ではなくレイアウトサービスの決めるコンテナ(実質document.body側)に
// 付け替えるため、`.monaco-editor`配下に限定せずグローバルに無効化する。
const style = document.createElement("style");
style.textContent = `.monaco-hover { display: none !important; }`;
document.head.appendChild(style);

const baseTheme: Pick<monaco.editor.IStandaloneThemeData, "base" | "inherit" | "rules"> = {
  base: "vs",
  inherit: true,
  rules: [],
};

monaco.editor.defineTheme("sensitivemasker-input", {
  ...baseTheme,
  colors: {
    "editor.background": "#fafafa",
    "editor.lineHighlightBackground": "#f0f0f0",
    // 既定の行番号色は背景色との組み合わせでWCAG AA基準を満たさないため固定する。
    "editorLineNumber.foreground": "#666666",
  },
});

monaco.editor.defineTheme("sensitivemasker-output", {
  ...baseTheme,
  colors: {
    "editor.background": "#f0f0f0",
    "editor.lineHighlightBackground": "#e8e8e8",
    "editorLineNumber.foreground": "#666666",
  },
});
