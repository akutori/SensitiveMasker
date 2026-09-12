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

function profileRow(name: string) {
  return $(`//div[contains(@class,"rounded-lg")][.//*[contains(text(),"${name}")]]`);
}

describe("プロファイル管理画面でのCRUD操作", () => {
  it("新規作成・お気に入り切り替え・複製・削除が一通り動く", async () => {
    await completeInitialSetup();

    const uniqueName = `E2Eプロファイル${Date.now()}`;

    const newButton = await $("button=新規作成");
    await newButton.waitForExist({ timeout: 10000 });
    await newButton.click();

    const nameDialog = await $('[role="dialog"]');
    await nameDialog.waitForExist({ timeout: 10000 });
    const nameInput = await nameDialog.$("input");
    await nameInput.setValue(uniqueName);
    const createOkButton = await nameDialog.$("button=OK");
    await createOkButton.click();
    await nameDialog.waitForExist({ timeout: 10000, reverse: true });

    // 新規作成すると、ルール編集画面(/rules/$profileId)へ自動遷移する。
    // 何も編集していない状態の「キャンセル」は確認無しでメイン画面へ戻る。
    const cancelButton = await $("button=キャンセル");
    await cancelButton.waitForExist({ timeout: 10000 });
    await cancelButton.click();

    const listButton = await $("button=プロファイル一覧");
    await listButton.waitForExist({ timeout: 10000 });
    await listButton.click();
    await $("h1=プロファイル管理").waitForExist({ timeout: 10000 });

    const row = await profileRow(uniqueName);
    await row.waitForExist({ timeout: 10000 });

    // お気に入り切り替え(トグル後、aria-labelが反転することで確認する)
    const favoriteButton = await row.$('button[aria-label="お気に入りに追加"]');
    await favoriteButton.waitForExist({ timeout: 10000 });
    await favoriteButton.click();
    const unfavoriteButton = await row.$('button[aria-label="お気に入りから外す"]');
    await unfavoriteButton.waitForExist({ timeout: 10000 });

    // 複製(既定名は「元の名前 のコピー」)
    const duplicateButton = await row.$("button=複製");
    await duplicateButton.click();
    const duplicateDialog = await $('[role="dialog"]');
    await duplicateDialog.waitForExist({ timeout: 10000 });
    const duplicateOkButton = await duplicateDialog.$("button=OK");
    await duplicateOkButton.click();
    await duplicateDialog.waitForExist({ timeout: 10000, reverse: true });

    const duplicateName = `${uniqueName} のコピー`;
    const duplicateRow = await profileRow(duplicateName);
    await duplicateRow.waitForExist({ timeout: 10000 });

    // 複製(非アクティブ)を削除する。アクティブな元プロファイルの「削除」は
    // disabledのため対象にしない(profile-store側で別途テスト済み)。
    const deleteButton = await duplicateRow.$("button=削除");
    await deleteButton.click();
    const deleteDialog = await $('[role="alertdialog"]');
    await deleteDialog.waitForExist({ timeout: 10000 });
    const yesButton = await deleteDialog.$("button=はい");
    await yesButton.click();

    await duplicateRow.waitForExist({ timeout: 10000, reverse: true });
  });
});
