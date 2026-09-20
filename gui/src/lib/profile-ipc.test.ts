import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauriCore = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => tauriCore);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { listen } from "@tauri-apps/api/event";
import {
  clearPendingImport,
  commitPendingImport,
  onMainWindowHiddenToTray,
  previewImport,
} from "./profile-ipc";

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
    expect(result).toEqual({ pendingId: 12, preview: DUMMY_PREVIEW, passphraseTrimmed: false });
  });

  it("previewImportは、応答のpassphrase_trimmed(前後の空白を除いて復号した)を、passphraseTrimmedとして返す", async () => {
    tauriCore.invoke.mockResolvedValue({ pending_id: 12, preview: DUMMY_PREVIEW, passphrase_trimmed: true });

    const result = await previewImport("C:/dummy/export.smx", "dummy-passphrase-0001");

    expect(result.passphraseTrimmed).toBe(true);
  });

  it.each([
    ["falseのとき", { pending_id: 12, preview: DUMMY_PREVIEW, passphrase_trimmed: false }],
    ["無いとき(取り決めがずれても、確認画面へ進む流れは止めない。知らせないだけ)", { pending_id: 12, preview: DUMMY_PREVIEW }],
    ["真偽値でないとき", { pending_id: 12, preview: DUMMY_PREVIEW, passphrase_trimmed: "true" }],
  ])("previewImportは、passphrase_trimmedが%s、passphraseTrimmedをfalseにする", async (_label, response) => {
    tauriCore.invoke.mockResolvedValue(response);

    const result = await previewImport("C:/dummy/export.smx", "dummy-passphrase-0001");

    expect(result.passphraseTrimmed).toBe(false);
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

// Rust側(tray.rsのMAIN_WINDOW_HIDDEN_EVENT)が、メインウィンドウのトレイへの格納を知らせるイベントの名前を固定する。
describe("トレイへの格納の通知", () => {
  it("onMainWindowHiddenToTrayは、Rust側が知らせるイベント名を購読し、購読の解除を返す", async () => {
    const unlisten = vi.fn();
    vi.mocked(listen).mockResolvedValue(unlisten);
    const handler = vi.fn();

    const stop = await onMainWindowHiddenToTray(handler);

    expect(listen).toHaveBeenCalledWith("main-window-hidden-to-tray", handler);
    expect(stop).toBe(unlisten);
  });
});

// 保留の識別子は、Rust側のu64で、0から払い出される。JavaScriptの数値として正確に扱える、0以上の安全な整数だけを有効とする。
// Rustのclear_pending_importは、識別子が無い・nullなら、全ての保留(他の画面が始めた復号の保留も)を破棄する。
// undefinedはキーごと落ち、NaN・Infinityはnullに直列化されるため、これらの識別子のままinvokeを呼ぶと、無言で全ての保留が消える。
// そのため、無効な識別子は、invokeを呼ばずに拒否する。
const INVALID_PENDING_IDS: Array<[string, unknown]> = [
  ["undefined", undefined],
  ["null", null],
  ["NaN", Number.NaN],
  ["Infinity", Number.POSITIVE_INFINITY],
  ["負数", -1],
  ["小数", 1.5],
  ["安全な整数を超える値", Number.MAX_SAFE_INTEGER + 1],
  ["数値でない値(文字列)", "7"],
];
const VALID_PENDING_IDS = [0, 12, Number.MAX_SAFE_INTEGER];

describe("保留の識別子の検証", () => {
  beforeEach(() => {
    tauriCore.invoke.mockReset();
  });

  it.each(INVALID_PENDING_IDS)("commitPendingImportは、識別子が%sなら、invokeを呼ばずに拒否する", async (_label, pendingId) => {
    await expect(commitPendingImport(pendingId as number)).rejects.toThrow(/invalid pending import id/);
    expect(tauriCore.invoke).not.toHaveBeenCalled();
  });

  it.each(INVALID_PENDING_IDS)("clearPendingImportは、識別子が%sなら、invokeを呼ばずに拒否する(識別子の無い呼び出しは、全ての保留を破棄してしまう)", async (_label, pendingId) => {
    await expect(clearPendingImport(pendingId as number)).rejects.toThrow(/invalid pending import id/);
    expect(tauriCore.invoke).not.toHaveBeenCalled();
  });

  it.each(VALID_PENDING_IDS)("識別子%iは有効として、commitPendingImportとclearPendingImportが、そのままinvokeへ渡す(0も有効)", async (pendingId) => {
    tauriCore.invoke.mockResolvedValue({ activated_profile_name: null });

    await commitPendingImport(pendingId);
    await clearPendingImport(pendingId);

    expect(tauriCore.invoke).toHaveBeenNthCalledWith(1, "commit_pending_import", { pendingId });
    expect(tauriCore.invoke).toHaveBeenNthCalledWith(2, "clear_pending_import", { pendingId });
  });

  it.each(VALID_PENDING_IDS)("previewImportは、応答のpending_idが%iなら、そのまま識別子として返す(0も有効)", async (pendingId) => {
    tauriCore.invoke.mockResolvedValue({ pending_id: pendingId, preview: DUMMY_PREVIEW });

    const result = await previewImport("C:/dummy/export.smx", "dummy-passphrase-0001");

    expect(result.pendingId).toBe(pendingId);
  });

  // 応答の形は、型引数で断定できるだけで、Rust側との取り決めがずれると、識別子が欠ける。
  it.each([
    ...INVALID_PENDING_IDS.map(([label, pendingId]): [string, unknown] => [
      `pending_idが${label}`,
      { pending_id: pendingId, preview: DUMMY_PREVIEW },
    ]),
    ["pending_idが無い", { preview: DUMMY_PREVIEW }],
    ["応答がnull", null],
    ["応答が空(undefined)", undefined],
  ])("previewImportは、応答が「%s」のとき、エラーを投げる(識別子の無い保留を、確認画面へ渡さない)", async (_label, response) => {
    tauriCore.invoke.mockResolvedValue(response);

    await expect(previewImport("C:/dummy/export.smx", "dummy-passphrase-0001")).rejects.toThrow(
      /invalid pending_id/
    );
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

  it("E2Eビルドでも、識別子が不正な応答のときは、識別子を残さない", async () => {
    const fakeWindow = inBuild("true");
    tauriCore.invoke.mockResolvedValue({ pending_id: Number.NaN, preview: DUMMY_PREVIEW });

    await expect(previewImport("C:/dummy/a.smx", "dummy-passphrase-0001")).rejects.toBeDefined();

    expect(fakeWindow.__e2ePendingImportIds).toBeUndefined();
  });

  it("E2Eビルドでは、識別子0も、残す(0を、識別子が無いことと取り違えない)", async () => {
    const fakeWindow = inBuild("true");
    tauriCore.invoke.mockResolvedValue({ pending_id: 0, preview: DUMMY_PREVIEW });

    await previewImport("C:/dummy/a.smx", "dummy-passphrase-0001");

    expect(fakeWindow.__e2ePendingImportIds).toEqual([0]);
  });

  it("E2Eビルドでなければ、previewImportは、識別子を、ページから読める場所へ残さない", async () => {
    const fakeWindow = inBuild(undefined);
    tauriCore.invoke.mockResolvedValue({ pending_id: 3, preview: DUMMY_PREVIEW });

    await previewImport("C:/dummy/a.smx", "dummy-passphrase-0001");

    expect(fakeWindow.__e2ePendingImportIds).toBeUndefined();
  });
});
