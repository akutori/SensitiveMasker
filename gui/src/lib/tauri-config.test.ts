import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// tauri.conf.jsonの設定のうち、画面の安全性に関わり、外れても画面から気づけないものを固定する。
describe("メインウィンドウの設定(tauri.conf.json)", () => {
  const config = JSON.parse(
    readFileSync(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8")
  ) as { app: { windows: Array<Record<string, unknown>> } };

  // WebView2の自動補完(Suggestions)は、入力欄のautocomplete="off"を守らない場合がある。
  // パスフレーズや、ルールのパターンなどの機微な入力が候補として残らないよう、窓ごと無効にする。
  it("WebView2の自動補完(Suggestions)を無効にしている", () => {
    expect(config.app.windows[0].generalAutofillEnabled).toBe(false);
  });
});
