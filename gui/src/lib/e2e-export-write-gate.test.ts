import { afterEach, describe, expect, it, vi } from "vitest";
import { waitForE2eExportWriteGate } from "./e2e-export-write-gate";

interface FakeWindow {
  __e2eExportWriteGate?: Promise<void>;
  __e2eExportWriteAttempts?: number;
}

// Promiseの後続が動くまで待つ。
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

// 書き込みは、E2Eビルドでだけ、テストが指定した待ちを通る(本番の書き込みを、ページ内のスクリプトが止められないように)。
describe("E2E用の、エクスポートの書き込みの口", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  function inBuild(env: string | undefined, gate?: Promise<void>) {
    const fakeWindow: FakeWindow = { __e2eExportWriteGate: gate };
    vi.stubGlobal("window", fakeWindow);
    if (env === undefined) vi.stubEnv("VITE_E2E_TESTING", undefined);
    else vi.stubEnv("VITE_E2E_TESTING", env);
    return fakeWindow;
  }

  // 決着しない待ち。口が待つなら、この待ちの間、waitForE2eExportWriteGateは終わらない。
  const neverSettles = () => new Promise<void>(() => {});

  it("E2Eビルドでなければ、指定された待ちがあっても待たず、何も数えない", async () => {
    const fakeWindow = inBuild(undefined, neverSettles());

    await waitForE2eExportWriteGate();

    expect(fakeWindow.__e2eExportWriteAttempts).toBeUndefined();
  });

  it('環境変数が"true"以外(例: "false")なら、待たず、何も数えない', async () => {
    const fakeWindow = inBuild("false", neverSettles());

    await waitForE2eExportWriteGate();

    expect(fakeWindow.__e2eExportWriteAttempts).toBeUndefined();
  });

  it("E2Eビルドでは、指定が無ければ、待たずに通り、通った回数を数える", async () => {
    const fakeWindow = inBuild("true");

    await waitForE2eExportWriteGate();
    await waitForE2eExportWriteGate();

    expect(fakeWindow.__e2eExportWriteAttempts).toBe(2);
  });

  it("E2Eビルドでは、指定された待ちが終わるまで待つ(待つ間も、来たことは数える)", async () => {
    let release!: () => void;
    const fakeWindow = inBuild(
      "true",
      new Promise<void>((resolve) => {
        release = resolve;
      })
    );
    const settled = vi.fn();

    void waitForE2eExportWriteGate().then(settled);
    await flush();
    expect(settled).not.toHaveBeenCalled();
    expect(fakeWindow.__e2eExportWriteAttempts).toBe(1);

    release();
    await flush();
    expect(settled).toHaveBeenCalledTimes(1);
  });
});
