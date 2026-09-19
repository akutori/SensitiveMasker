import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const dialogPlugin = vi.hoisted(() => ({
  open: vi.fn(async () => "real-open-path"),
  save: vi.fn(async () => "real-save-path"),
}));
vi.mock("@tauri-apps/plugin-dialog", () => dialogPlugin);

import { openFileDialog, saveFileDialog } from "./file-dialog";

// E2Eビルド以外では、window.__e2eFileDialogPathsが設定されていても、ネイティブのダイアログを
// 差し替えない(本番の保存先・取り込み元の選択を、ページ内のスクリプトに握らせないため)。
describe("ファイルダイアログの差し替え口", () => {
  beforeEach(() => {
    dialogPlugin.open.mockClear();
    dialogPlugin.save.mockClear();
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  function withReplacement(env: string | undefined, paths: object) {
    vi.stubGlobal("window", { __e2eFileDialogPaths: paths });
    if (env === undefined) vi.stubEnv("VITE_E2E_TESTING", undefined);
    else vi.stubEnv("VITE_E2E_TESTING", env);
  }

  it("E2Eビルドでなければ、指定があっても本物の保存ダイアログを使う", async () => {
    withReplacement(undefined, { save: "replaced-save-path" });

    expect(await saveFileDialog()).toBe("real-save-path");
    expect(dialogPlugin.save).toHaveBeenCalledTimes(1);
  });

  it("E2Eビルドでなければ、指定があっても本物の開くダイアログを使う", async () => {
    withReplacement(undefined, { open: "replaced-open-path" });

    expect(await openFileDialog()).toBe("real-open-path");
    expect(dialogPlugin.open).toHaveBeenCalledTimes(1);
  });

  it('環境変数が"true"以外(例: "false")なら、差し替えない', async () => {
    withReplacement("false", { save: "replaced-save-path" });

    expect(await saveFileDialog()).toBe("real-save-path");
    expect(dialogPlugin.save).toHaveBeenCalledTimes(1);
  });

  it("E2Eビルドでは、指定したパスを返し、本物のダイアログを開かない", async () => {
    withReplacement("true", { save: "replaced-save-path", open: "replaced-open-path" });

    expect(await saveFileDialog()).toBe("replaced-save-path");
    expect(await openFileDialog()).toBe("replaced-open-path");
    expect(dialogPlugin.save).not.toHaveBeenCalled();
    expect(dialogPlugin.open).not.toHaveBeenCalled();
  });

  it("E2Eビルドでは、保存ダイアログの取り消し(null)も、本物のダイアログへ進まず返す", async () => {
    withReplacement("true", { save: Promise.resolve(null) });

    expect(await saveFileDialog()).toBeNull();
    expect(dialogPlugin.save).not.toHaveBeenCalled();
  });

  it("E2Eビルドでも、指定が無い側は本物のダイアログを使う", async () => {
    withReplacement("true", { open: "replaced-open-path" });

    expect(await saveFileDialog()).toBe("real-save-path");
    expect(dialogPlugin.save).toHaveBeenCalledTimes(1);
  });
});
