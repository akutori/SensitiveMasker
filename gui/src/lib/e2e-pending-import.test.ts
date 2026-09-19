import { afterEach, describe, expect, it, vi } from "vitest";
import { recordPendingImportIdForE2e } from "./e2e-pending-import";

// E2Eビルド以外では、受け取った保留の識別子を、ページから読める場所へ残さない
// (本番の保留の識別子を、ページ内のスクリプトに見せないため)。
describe("E2E用の、受け取った保留の識別子の記録口", () => {
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

  it("E2Eビルドでなければ、何も残さない", () => {
    const fakeWindow = inBuild(undefined);

    recordPendingImportIdForE2e(5);
    expect(fakeWindow.__e2ePendingImportIds).toBeUndefined();
  });

  it('環境変数が"true"以外(例: "false")なら、何も残さない', () => {
    const fakeWindow = inBuild("false");

    recordPendingImportIdForE2e(5);
    expect(fakeWindow.__e2ePendingImportIds).toBeUndefined();
  });

  it("E2Eビルドでは、受け取った順に、識別子を残す", () => {
    const fakeWindow = inBuild("true");

    recordPendingImportIdForE2e(5);
    recordPendingImportIdForE2e(6);
    expect(fakeWindow.__e2ePendingImportIds).toEqual([5, 6]);
  });

  it("E2Eビルドでは、テストが先に用意した一覧へも追記する(既にある識別子を消さない)", () => {
    const fakeWindow = inBuild("true");
    fakeWindow.__e2ePendingImportIds = [1];

    recordPendingImportIdForE2e(2);
    expect(fakeWindow.__e2ePendingImportIds).toEqual([1, 2]);
  });
});
