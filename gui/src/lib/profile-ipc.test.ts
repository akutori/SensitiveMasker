import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauriCore = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => tauriCore);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { clearPendingImport, commitPendingImport, previewImport } from "./profile-ipc";

const DUMMY_PREVIEW = {
  kind: "single",
  name: "ダミープロファイル",
  rules: [],
  tags: [],
} as const;

// Rust側のコマンド(preview_import・commit_pending_import・clear_pending_import)との、引数名・戻り値の形の取り決めを固定する。
// 確定・破棄は、復号のたびに払い出される識別子(引数pendingId)で、その保留だけを指す。
describe("インポートの保留のIPC", () => {
  beforeEach(() => {
    tauriCore.invoke.mockReset();
  });

  it("previewImportは、Rustの結果(pending_id・preview)を、pendingIdとpreviewに詰め替えて返す", async () => {
    tauriCore.invoke.mockResolvedValue({ pending_id: 12, preview: DUMMY_PREVIEW });

    const result = await previewImport("C:/dummy/export.smx", "dummy-passphrase-0001");

    expect(tauriCore.invoke).toHaveBeenCalledWith("preview_import", {
      sourcePath: "C:/dummy/export.smx",
      passphrase: "dummy-passphrase-0001",
    });
    expect(result).toEqual({ pendingId: 12, preview: DUMMY_PREVIEW });
  });

  it("previewImportは、失敗(復号できないなど)を、そのまま伝える", async () => {
    tauriCore.invoke.mockRejectedValue({ kind: "failed", message: "dummy failure" });

    await expect(previewImport("C:/dummy/export.smx", "dummy-passphrase-0001")).rejects.toEqual({
      kind: "failed",
      message: "dummy failure",
    });
  });

  it("commitPendingImportは、指定した識別子(pendingId)で、確定を呼ぶ", async () => {
    tauriCore.invoke.mockResolvedValue({ activated_profile_name: null });

    await commitPendingImport(12);

    expect(tauriCore.invoke).toHaveBeenCalledWith("commit_pending_import", { pendingId: 12 });
  });

  it("clearPendingImportは、指定した識別子(pendingId)だけを指して、破棄を呼ぶ", async () => {
    tauriCore.invoke.mockResolvedValue(undefined);

    await clearPendingImport(12);

    expect(tauriCore.invoke).toHaveBeenCalledWith("clear_pending_import", { pendingId: 12 });
  });
});

// E2Eビルド(VITE_E2E_TESTING)に限り、previewImportが受け取った識別子を、E2Eテストが読める場所へ残す。
describe("インポートの保留の識別子の、E2E用の記録", () => {
  beforeEach(() => {
    tauriCore.invoke.mockReset();
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  function inBuild(env: string | undefined) {
    const fakeWindow: { __e2ePendingImportIds?: number[] } = {};
    vi.stubGlobal("window", fakeWindow);
    if (env === undefined) vi.stubEnv("VITE_E2E_TESTING", undefined);
    else vi.stubEnv("VITE_E2E_TESTING", env);
    return fakeWindow;
  }

  it("E2Eビルドでは、previewImportが受け取った識別子を、受け取った順に残す", async () => {
    const fakeWindow = inBuild("true");
    tauriCore.invoke.mockResolvedValueOnce({ pending_id: 3, preview: DUMMY_PREVIEW });
    tauriCore.invoke.mockResolvedValueOnce({ pending_id: 4, preview: DUMMY_PREVIEW });

    await previewImport("C:/dummy/a.smx", "dummy-passphrase-0001");
    await previewImport("C:/dummy/b.smx", "dummy-passphrase-0002");

    expect(fakeWindow.__e2ePendingImportIds).toEqual([3, 4]);
  });

  it("E2Eビルドでも、previewImportが失敗したときは、識別子を残さない", async () => {
    const fakeWindow = inBuild("true");
    tauriCore.invoke.mockRejectedValue({ kind: "failed", message: "dummy failure" });

    await expect(previewImport("C:/dummy/a.smx", "dummy-passphrase-0001")).rejects.toBeDefined();

    expect(fakeWindow.__e2ePendingImportIds).toBeUndefined();
  });

  it("E2Eビルドでなければ、previewImportは、識別子を、ページから読める場所へ残さない", async () => {
    const fakeWindow = inBuild(undefined);
    tauriCore.invoke.mockResolvedValue({ pending_id: 3, preview: DUMMY_PREVIEW });

    await previewImport("C:/dummy/a.smx", "dummy-passphrase-0001");

    expect(fakeWindow.__e2ePendingImportIds).toBeUndefined();
  });
});
