import { describe, expect, it } from "vitest";
import {
  abortExport,
  beginExport,
  canRegenerate,
  canStartExport,
  completeExport,
  isPassphraseAtRisk,
  openExportDialog,
  regeneratePassphrase,
  type ExportDialogState,
} from "./export-dialog-state";

const PASSPHRASE = "dummy-passphrase-0001";
const NEXT_PASSPHRASE = "dummy-passphrase-0002";

const editing = (sessionId = 1): ExportDialogState => ({
  phase: "editing",
  sessionId,
  passphrase: PASSPHRASE,
});
const exporting = (sessionId = 1): ExportDialogState => ({
  phase: "exporting",
  sessionId,
  passphrase: PASSPHRASE,
});
const exported = (sessionId = 1): ExportDialogState => ({
  phase: "exported",
  sessionId,
  passphrase: PASSPHRASE,
});

describe("openExportDialog", () => {
  it("編集中から始まり、渡された開いた回の番号とパスフレーズを持つ", () => {
    expect(openExportDialog(3, PASSPHRASE)).toEqual(editing(3));
  });
});

describe("regeneratePassphrase", () => {
  it("編集中ならパスフレーズだけが置き換わる", () => {
    expect(regeneratePassphrase(editing(2), NEXT_PASSPHRASE)).toEqual({
      phase: "editing",
      sessionId: 2,
      passphrase: NEXT_PASSPHRASE,
    });
  });

  it("実行中は変わらない(書き出すパスフレーズを途中で変えない)", () => {
    expect(regeneratePassphrase(exporting(), NEXT_PASSPHRASE)).toEqual(exporting());
  });

  it("成功後は変わらない(書き出したファイルと画面のパスフレーズを食い違わせない)", () => {
    expect(regeneratePassphrase(exported(), NEXT_PASSPHRASE)).toEqual(exported());
  });
});

describe("beginExport", () => {
  it("編集中で、開いた回の番号とパスフレーズが一致すれば、実行中になる", () => {
    expect(beginExport(editing(2), 2, PASSPHRASE)).toEqual(exporting(2));
  });

  it("開いた回の番号が異なれば変わらない(別の画面の状態を実行中にしない)", () => {
    expect(beginExport(editing(2), 1, PASSPHRASE)).toEqual(editing(2));
  });

  it("パスフレーズが異なれば変わらない(画面に出ているものと違うパスフレーズで書き出させない)", () => {
    expect(beginExport(editing(2), 2, NEXT_PASSPHRASE)).toEqual(editing(2));
  });

  it("実行中は変わらない(二重実行を防ぐ)", () => {
    expect(beginExport(exporting(), 1, PASSPHRASE)).toEqual(exporting());
  });

  it("成功後は変わらない(書き出し済みのパスフレーズで再実行させない)", () => {
    expect(beginExport(exported(), 1, PASSPHRASE)).toEqual(exported());
  });
});

describe("completeExport", () => {
  it("実行中で開いた回の番号が一致すれば、成功後になる(パスフレーズは保たれる)", () => {
    expect(completeExport(exporting(2), 2)).toEqual(exported(2));
  });

  it("開いた回の番号が異なれば変わらない(閉じて開き直した後に届いた、古い実行の完了通知)", () => {
    expect(completeExport(exporting(2), 1)).toEqual(exporting(2));
  });

  it("実行中でなければ変わらない", () => {
    expect(completeExport(editing(2), 2)).toEqual(editing(2));
    expect(completeExport(exported(2), 2)).toEqual(exported(2));
  });
});

describe("abortExport", () => {
  it("実行中で開いた回の番号が一致すれば、編集中へ戻る(保存先の選択の取り消し・失敗)", () => {
    expect(abortExport(exporting(2), 2)).toEqual(editing(2));
  });

  it("開いた回の番号が異なれば変わらない", () => {
    expect(abortExport(exporting(2), 1)).toEqual(exporting(2));
  });

  it("成功後は編集中へ戻らない(書き出し済みのファイルと画面のパスフレーズの対応を保つ)", () => {
    expect(abortExport(exported(2), 2)).toEqual(exported(2));
  });
});

describe("isPassphraseAtRisk", () => {
  it("編集中は、まだ何も書き出していないので、失っても失うものがない", () => {
    expect(isPassphraseAtRisk("editing")).toBe(false);
  });

  it("実行中と成功後は、書き出したファイルを開ける唯一の手がかりなので、失わせてはならない", () => {
    expect(isPassphraseAtRisk("exporting")).toBe(true);
    expect(isPassphraseAtRisk("exported")).toBe(true);
  });
});

describe("操作の可否", () => {
  it("再生成とエクスポートは、編集中だけ可能", () => {
    expect(canRegenerate(editing())).toBe(true);
    expect(canStartExport(editing())).toBe(true);
    for (const state of [exporting(), exported()]) {
      expect(canRegenerate(state)).toBe(false);
      expect(canStartExport(state)).toBe(false);
    }
  });
});
