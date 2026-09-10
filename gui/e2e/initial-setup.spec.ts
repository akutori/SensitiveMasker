describe("初回セットアップ", () => {
  it("「始める」を押すとメイン画面(マスク実行ボタン)へ遷移する", async () => {
    const startButton = await $("button=始める");
    await startButton.waitForExist({ timeout: 10000 });
    await startButton.click();

    const maskButton = await $("button*=マスク実行");
    await maskButton.waitForExist({ timeout: 10000 });
    await expect(maskButton).toBeDisplayed();
  });
});
