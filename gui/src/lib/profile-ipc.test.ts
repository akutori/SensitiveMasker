import { beforeEach, describe, expect, it, vi } from "vitest";

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
