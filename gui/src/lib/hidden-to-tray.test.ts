import { describe, expect, it } from "vitest";
import { actionOnHiddenToTray } from "./hidden-to-tray";

describe("actionOnHiddenToTray", () => {
  describe("パスフレーズ・復号済みの内容・.envの値を持つ画面は、閉じる", () => {
    it.each([
      "importPassphrase",
      "importConfirm",
      "envImportSelect",
      "envImportName",
    ])("%s", (kind) => {
      expect(actionOnHiddenToTray(kind)).toBe("close");
    });
  });

  describe("パスフレーズなどを持たない画面は、そのままにする", () => {
    it.each([
      "none",
      "newProfile",
      "templateSelect",
      "profileNameFromTemplate",
      "tagManagement",
      "fileImportChoice",
      "overwriteConfirm",
      "matchCountConfirm",
    ])("%s", (kind) => {
      expect(actionOnHiddenToTray(kind)).toBe("keep");
    });
  });

  it("知らない種類の画面は、閉じる(秘匿情報を持つ画面が、格納したまま残らないように)", () => {
    expect(actionOnHiddenToTray("aFutureDialog")).toBe("close");
  });

  describe("エクスポート画面は、局面ごとに扱いが違う", () => {
    it("編集中は、まだ何も書き出していないため、閉じる", () => {
      expect(actionOnHiddenToTray("export", "editing")).toBe("close");
    });

    it("保存先の選択中は、まだ何も書き出していないため、閉じる", () => {
      expect(actionOnHiddenToTray("export", "choosing")).toBe("close");
    });

    it("書き込み中は、閉じずに残す(閉じると、パスフレーズを失ったまま、ファイルだけが書き出される)", () => {
      expect(actionOnHiddenToTray("export", "writing")).toBe("keep");
    });

    it("書き出し済みは、閉じずに、表示を伏せ字へ戻す(閉じると、パスフレーズを二度と表示できない)", () => {
      expect(actionOnHiddenToTray("export", "exported")).toBe("conceal");
    });

    it("局面が分からないときは、閉じる", () => {
      expect(actionOnHiddenToTray("export")).toBe("close");
    });
  });

  it("エクスポート以外の画面は、局面を渡されても、種類だけで決まる", () => {
    expect(actionOnHiddenToTray("importConfirm", "exported")).toBe("close");
    expect(actionOnHiddenToTray("none", "writing")).toBe("keep");
  });
});
