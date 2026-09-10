// tauri.conf.jsonのcsp/devCspが実際の画面操作を壊していないかを検証する。
// securitypolicyviolationイベントはCSP違反発生時に必ずdocumentへ届くため、これを
// 収集して0件であることを確認する。特にMonaco Editor(main-screenの入出力欄、
// @monaco-editor/react)はworker生成やeval実行でCSPに抵触しやすいことが知られている
// ため、実際にマウントされた時点まで待ってから判定する。
// リスナーは「始める」クリック前(=main-screenが初めてマウントされCSPが評価される前)
// に仕込むことで、初回ロード時点の違反も取りこぼさないようにする。

describe("CSP", () => {
  it("初回セットアップ後のmain-screen(Monaco Editor含む)でCSP違反が発生しない", async () => {
    const startButton = await $("button=始める");
    let freshSetup = false;
    try {
      await startButton.waitForExist({ timeout: 5000 });
      freshSetup = true;
    } catch {
      // 既に初期化済み。
    }

    await browser.execute(() => {
      (window as unknown as { __cspViolations: unknown[] }).__cspViolations = [];
      document.addEventListener("securitypolicyviolation", (e) => {
        (window as unknown as { __cspViolations: unknown[] }).__cspViolations.push({
          directive: e.violatedDirective,
          blockedURI: e.blockedURI,
        });
      });
    });

    if (freshSetup) {
      await startButton.click();
    } else {
      await browser.refresh();
    }

    const maskButton = await $("button*=マスク実行");
    await maskButton.waitForExist({ timeout: 10000 });

    const monacoEditor = await $(".monaco-editor");
    await monacoEditor.waitForExist({ timeout: 15000 });

    const violations = await browser.execute(
      () => (window as unknown as { __cspViolations: unknown[] }).__cspViolations
    );
    expect(violations).toEqual([]);

    // 「違反が0件」だけでは「CSP自体が適用されていない」ケースと区別が付かないため、
    // 意図的にconnect-srcで許可していない外部オリジンへfetchし、実際に拒否される
    // ことも確認する(このテストがCSP不在のまま無条件に通ってしまうことを防ぐ)。
    const externalFetchBlocked = await browser.execute(async () => {
      try {
        await fetch("https://example.com/");
        return false;
      } catch {
        return true;
      }
    });
    expect(externalFetchBlocked).toBe(true);
  });
});
