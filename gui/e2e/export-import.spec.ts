// エクスポート/インポートの「エクスポート実行」「インポート」ボタンは、押した瞬間に
// OSネイティブのファイルダイアログ(plugin:dialog|save・plugin:dialog|open)を呼ぶ。
// これはWebDriver自動化の対象外であるだけでなく、実機検証の結果、このアプリの
// window.__TAURI_INTERNALS__.invoke呼び出しはbrowser.tauri.mock()からも、
// テストコード側で直接window.__TAURI_INTERNALS__.invokeを差し替える方法からも
// 傍受できないことを確認した(このアプリは@tauri-apps/api/coreをESモジュールで
// importしており、実行時に呼ばれるinvokeの実体が、embedded webdriverサーバーの
// executeScriptが操作できるコンテキストと分離しているとみられる)。差し替えられない
// まま「エクスポート実行」まで押すと、実際のOSネイティブダイアログが開いたまま
// 応答を待ち続けてテストがハングする(自動化環境には応答する相手がいないため)。
//
// そのため、このファイルでは「エクスポート実行」ボタン自体は押さず、ネイティブ
// ダイアログを一切呼ばない範囲(モーダルが実データで正しく開くこと)だけを検証する。
// バックエンドのexport/import本体のロジック(暗号化・復号・往復・エラー系)は
// gui/src-tauri/src/export_import.rsの実ファイル・実DBを使ったテストで別途検証済み。
// 実際にネイティブダイアログを操作しての最終確認は手動でのみ可能。

async function completeInitialSetup() {
  const startButton = await $("button=始める");
  try {
    await startButton.waitForExist({ timeout: 5000 });
    await startButton.click();
  } catch {
    // 既に初期化済み。
  }
  const maskButton = await $("button*=マスク実行");
  await maskButton.waitForExist({ timeout: 10000 });
}

async function createProfileViaIpc(name: string) {
  await browser.tauri.execute(
    ({ core }, profileName) =>
      core.invoke("create_profile", {
        profile: { profile_name: profileName, description: null, rules: [] },
      }),
    name
  );
}

describe("エクスポートモーダル", () => {
  it("プロファイル一覧の「エクスポート」から実データで生成されたパスフレーズ入りのモーダルが開く", async () => {
    await completeInitialSetup();
    await createProfileViaIpc("E2Eエクスポート表示確認");

    const listButton = await $("button=プロファイル一覧");
    await listButton.waitForExist({ timeout: 10000 });
    await listButton.click();
    await $("h1=プロファイル管理").waitForExist({ timeout: 10000 });

    const row = await $(
      `//div[contains(@class,"rounded-lg")][.//*[contains(text(),"E2Eエクスポート表示確認")]]`
    );
    const exportButton = await row.$("button=エクスポート");
    await exportButton.waitForExist({ timeout: 10000 });
    await exportButton.click();

    const dialog = await $('[role="dialog"]');
    await dialog.waitForExist({ timeout: 10000 });
    const dialogText = await dialog.getText();
    expect(dialogText).toContain("E2Eエクスポート表示確認");

    const passphraseInput = await dialog.$("input[readonly]");
    await passphraseInput.waitForExist({ timeout: 10000 });
    const passphrase = await passphraseInput.getValue();
    expect(passphrase.length).toBeGreaterThan(0);

    // 「エクスポート」(確認)は押さない: 押すとOSネイティブの保存ダイアログが
    // 開いたまま応答を待ち続け、この自動化環境では誰も応答できずハングする。
    const cancelButton = await dialog.$("button=キャンセル");
    await cancelButton.click();
  });
});
