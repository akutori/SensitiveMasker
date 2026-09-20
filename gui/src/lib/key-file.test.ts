import { describe, expect, it } from "vitest";
import { hasKeyFileExtension, keyFileExportBaseName, keyFileNameOf, pickKeyFileFromDroppedPaths } from "./key-file";

describe("keyFileExportBaseName", () => {
  // 月は0始まりのため、8は9月。
  const at = new Date(2026, 8, 20, 21, 5, 9);

  it("プロファイル1件のエクスポートは、日時(年月日-時分秒。ゼロ埋め)を含む名前になる", () => {
    expect(keyFileExportBaseName(false, at)).toBe("sensitivemasker_export_20260920-210509");
  });

  it("全プロファイルのエクスポートは、別の名前になる", () => {
    expect(keyFileExportBaseName(true, at)).toBe("sensitivemasker_all_20260920-210509");
  });

  it("日時が違えば、名前も違う(前のエクスポートを、置き換えにくい)", () => {
    expect(keyFileExportBaseName(false, new Date(2026, 8, 20, 21, 5, 10))).not.toBe(keyFileExportBaseName(false, at));
  });
});

describe("keyFileNameOf", () => {
  it("WindowsとUnixの区切りの、最後の要素を返す", () => {
    expect(keyFileNameOf("C:\\dummy\\keys\\export.smxkey")).toBe("export.smxkey");
    expect(keyFileNameOf("/dummy/keys/export.smxkey")).toBe("export.smxkey");
    expect(keyFileNameOf("export.smxkey")).toBe("export.smxkey");
  });
});

describe("hasKeyFileExtension", () => {
  it.each(["C:\\dummy\\a.smxkey", "/dummy/a.smxkey", "a.smxkey", "a.SMXKEY", "a.b.smxkey"])(
    "%s は、鍵ファイルの拡張子",
    (path) => {
      expect(hasKeyFileExtension(path)).toBe(true);
    }
  );

  it.each([
    ["別の拡張子", "a.smx"],
    ["拡張子が無い", "smxkey"],
    ["名前が無い(拡張子だけ)", ".smxkey"],
    ["拡張子が、名前の一部", "a.smxkey.txt"],
    ["拡張子に、余分な文字", "a.smxkeys"],
    ["区切りの後が、拡張子だけ", "C:\\dummy\\.smxkey"],
  ])("%s(%s)は、鍵ファイルの拡張子ではない", (_label, path) => {
    expect(hasKeyFileExtension(path)).toBe(false);
  });
});

describe("pickKeyFileFromDroppedPaths", () => {
  it("ちょうど1つの.smxkeyのファイルなら、そのパスを選ぶ", () => {
    expect(pickKeyFileFromDroppedPaths(["C:\\dummy\\a.smxkey"])).toEqual({ kind: "picked", path: "C:\\dummy\\a.smxkey" });
  });

  it("何もドロップされていなければ、拒否する", () => {
    expect(pickKeyFileFromDroppedPaths([])).toMatchObject({ kind: "rejected" });
  });

  it("複数のファイルなら、鍵ファイルが含まれていても、選ばずに拒否する", () => {
    const pick = pickKeyFileFromDroppedPaths(["C:\\dummy\\a.smxkey", "C:\\dummy\\b.smxkey"]);
    expect(pick).toMatchObject({ kind: "rejected" });
    expect(pickKeyFileFromDroppedPaths(["C:\\dummy\\a.smxkey", "C:\\dummy\\b.txt"])).toMatchObject({ kind: "rejected" });
  });

  it("拡張子が違えば、拒否する", () => {
    const pick = pickKeyFileFromDroppedPaths(["C:\\dummy\\a.smx"]);
    expect(pick).toEqual({ kind: "rejected", message: "拡張子が.smxkeyのファイルをドロップしてください" });
  });
});
