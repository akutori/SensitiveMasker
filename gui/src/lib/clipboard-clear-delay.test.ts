import { describe, expect, it } from "vitest";
import { CLIPBOARD_CLEAR_DELAY_MS, CLIPBOARD_CLEAR_DELAY_SECONDS } from "./clipboard-clear-delay";

describe("自動クリアまでの時間", () => {
  it("表示用の秒数は、タイマーの時間から導出した整数である(「45.5秒後」のような表示にしない)", () => {
    expect(CLIPBOARD_CLEAR_DELAY_SECONDS * 1000).toBe(CLIPBOARD_CLEAR_DELAY_MS);
    expect(Number.isInteger(CLIPBOARD_CLEAR_DELAY_SECONDS)).toBe(true);
  });
});
