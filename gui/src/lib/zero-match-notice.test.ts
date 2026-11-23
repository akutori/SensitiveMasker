import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("sonner", () => ({ toast: { warning: vi.fn() } }));

import { toast } from "sonner";
import {
  notifyZeroMatchRules,
  zeroMatchRulesMessage,
  ZERO_MATCH_TOAST_DURATION_MS,
} from "./zero-match-notice";

describe("notifyZeroMatchRules", () => {
  beforeEach(() => {
    vi.mocked(toast.warning).mockClear();
  });

  it("0件のルールだけを、読点区切りで文言に入れて、警告のトーストを出す", () => {
    notifyZeroMatchRules([
      { ruleName: "jp_phone_number", count: 0 },
      { ruleName: "ipv4_address", count: 3 },
      { ruleName: "password_kv", count: 0 },
    ]);

    expect(toast.warning).toHaveBeenCalledTimes(1);
    expect(vi.mocked(toast.warning).mock.calls[0][0]).toBe(
      "マッチ件数が0件のルールがあります(jp_phone_number、password_kv)。意図した設定でない場合はルール設定を見直してください。"
    );
  });

  it("閉じるボタンと自動で閉じる時間を付け、右寄せのボタンを覆わない左下に出す", () => {
    notifyZeroMatchRules([{ ruleName: "jp_phone_number", count: 0 }]);

    expect(vi.mocked(toast.warning).mock.calls[0][1]).toEqual({
      closeButton: true,
      duration: 5000,
      position: "bottom-left",
    });
    expect(ZERO_MATCH_TOAST_DURATION_MS).toBe(5000);
  });

  it("全てのルールにマッチがあれば、トーストを出さない", () => {
    notifyZeroMatchRules([
      { ruleName: "jp_phone_number", count: 1 },
      { ruleName: "ipv4_address", count: 2 },
    ]);

    expect(toast.warning).not.toHaveBeenCalled();
  });

  it("ルールが無ければ、トーストを出さない", () => {
    notifyZeroMatchRules([]);

    expect(toast.warning).not.toHaveBeenCalled();
  });
});

describe("zeroMatchRulesMessage", () => {
  it("1件でも、読点なしで括弧に入れる", () => {
    expect(zeroMatchRulesMessage(["email_address"])).toBe(
      "マッチ件数が0件のルールがあります(email_address)。意図した設定でない場合はルール設定を見直してください。"
    );
  });
});
