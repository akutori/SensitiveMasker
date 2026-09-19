// メイン画面の「再読み込み」「クリップボードにコピー」を実IPCで検証する。
// 「ファイルから」「ファイルに保存」「直接マスクして別ファイルに保存」は、押した瞬間に
// OSネイティブのファイルダイアログ(plugin:dialog|open・plugin:dialog|save)を呼ぶ。
// ネイティブダイアログはWebDriverから操作できず、応答を待ち続けてハングするため、このファイルでは
// 検証しない。E2Eビルド限定の差し替え口(gui/src/lib/file-dialog.ts。使い方は
// export-import.spec.tsを参照)を使えば検証できるが、まだ扱っていない。
//
// クリップボードの実際の中身の読み取りは、plugin:clipboard-manager|read_textが
// このアプリのACL上フロントエンドから呼べない(lib.rsのコメント参照)ため、
// browser.tauri.execute経由でも読み取れない。そのため「コピーしました」トーストの
// 出現(=クリック→IPC呼び出し→成功、という配線が実際に繋がっていること)のみを
// 検証する(OSクリップボードの最終的な中身そのものは検証できない)。
//
// 「再読み込み」は、他プロセス(CLI等)がDBを直接書き換えた場合にこのウィンドウへ
// 反映するためのボタンである。browser.tauri.execute経由のcore.invoke呼び出しは、
// このアプリ自身のTauriコマンド(例: create_profile)を同じプロセス内で叩くだけであり、
// それらのコマンドは呼び出し経路によらずapp.emit("profiles-changed", ())を発行するため、
// 既存のイベント購読(app-state.tsxのonProfilesChanged)だけで既に画面へ反映されてしまい、
// 「再読み込みボタン自体が無ければ反映されない」ことの検証にならない。そのため、実際に別プロセスとして
// ビルド済みのmasker CLIバイナリを起動し、同じSENSITIVEMASKER_DATA_DIRへ向けて
// `masker profile create`を実行することで、GUIのイベントバスが一切関与しない、
// 本物の「外部プロセスによる変更」を再現する。

import { execFileSync } from "node:child_process";
import path from "node:path";

const MASKER_CLI_PATH = path.resolve(__dirname, "../../target/debug/masker.exe");

function createProfileViaExternalCli(name: string) {
  const dataDir = process.env.SENSITIVEMASKER_DATA_DIR;
  if (!dataDir) {
    throw new Error(
      "SENSITIVEMASKER_DATA_DIRが設定されていない(wdio runをこの変数付きで起動する必要がある)"
    );
  }
  execFileSync(MASKER_CLI_PATH, ["profile", "create", name], {
    env: { ...process.env, SENSITIVEMASKER_DATA_DIR: dataDir },
  });
}

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

// profile-management.spec.tsと同じ行検出パターン(プロファイル管理画面は素のdiv行で
// 描画される)。メイン画面のプロファイル選択はRadix Select(ネイティブ<select>ではない)
// のため、その内部構造(開くまでoptionが描画されない等)に依存しない、この既に実績のある
// 経路で「再読み込み」の効果を確認する。
function profileRow(name: string) {
  return $(`//div[contains(@class,"rounded-lg")][.//*[contains(text(),"${name}")]]`);
}

describe("メイン画面の再読み込み", () => {
  it("外部プロセス(masker CLI)で作成したプロファイルが、再読み込みボタンで一覧に反映される", async () => {
    await completeInitialSetup();

    const uniqueName = `E2E再読み込み確認${Date.now()}`;
    createProfileViaExternalCli(uniqueName);

    // "プロファイル一覧"ボタンは画面遷移のたびに要素参照を取り直す(閉じる→再読み込み→
    // 再度開くの間でDOMが作り直され、古い参照を使い回すとstale elementとしてクリックが
    // 効かなくなる)。
    await (await $("button=プロファイル一覧")).click();
    await $("h1=プロファイル管理").waitForExist({ timeout: 10000 });

    // 再読み込みを押す前は、直接IPCで作成した分はまだ見えていないはず
    // (このボタン自体の実装意義そのものの確認)。
    const rowBeforeReload = await profileRow(uniqueName);
    expect(await rowBeforeReload.isExisting()).toBe(false);

    await (await $("button=閉じる(メイン画面へ)")).click();
    await (await $("button=再読み込み")).waitForExist({ timeout: 10000 });
    await (await $("button=再読み込み")).click();

    await (await $("button=プロファイル一覧")).click();
    await $("h1=プロファイル管理").waitForExist({ timeout: 10000 });
    const rowAfterReload = await profileRow(uniqueName);
    await rowAfterReload.waitForExist({
      timeout: 10000,
      timeoutMsg: `再読み込み後もプロファイル「${uniqueName}」が一覧に現れなかった`,
    });

    // このwdio runプロセス内で後続に実行される他のit()(例:下のクリップボード
    // テスト)がcompleteInitialSetup()経由でメイン画面前提のまま始められるよう、
    // 画面遷移した分は必ずメイン画面へ戻しておく(spec単位でアプリが再起動される
    // わけではなく状態が共有され続けるため)。
    await (await $("button=閉じる(メイン画面へ)")).click();
    await (await $("button=再読み込み")).waitForExist({ timeout: 10000 });
  });
});

describe("メイン画面のクリップボードにコピー", () => {
  it("クリップボードにコピーを押すと成功トーストが出る", async () => {
    // 入力欄はMonaco Editorで、WebDriverからの直接のテキスト入力が難しいことが
    // 既知の制約として記録されている(過去のE2E拡充時、マスク実行フロー自体のE2E化を
    // 見送った経緯がある)。「クリップボードにコピー」ボタン自体はoutputTextが空でも
    // 押せる(マスク実行を前提にしない)ため、ここではマスク実行を経由せず、
    // ボタン→IPC呼び出し→成功、という配線そのものだけを検証する。
    await completeInitialSetup();

    const copyButton = await $("button=クリップボードにコピー");
    await copyButton.waitForExist({ timeout: 10000 });
    await copyButton.click();

    const toast = await $("div*=クリップボードにコピーしました");
    await toast.waitForExist({ timeout: 10000 });
  });
});
