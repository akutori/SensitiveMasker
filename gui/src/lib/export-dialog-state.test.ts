import { describe, expect, it } from "vitest";
import {
  abortExport,
  beginExport,
  beginWriting,
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
const choosing = (sessionId = 1): ExportDialogState => ({
  phase: "choosing",
  sessionId,
  passphrase: PASSPHRASE,
});
const writing = (sessionId = 1): ExportDialogState => ({
  phase: "writing",
  sessionId,
  passphrase: PASSPHRASE,
});
const exported = (sessionId = 1): ExportDialogState => ({
  phase: "exported",
  sessionId,
  passphrase: PASSPHRASE,
});

// 実行中(保存先の選択中と書き込み中)の2つの局面。
const inProgress = [
  ["保存先の選択中", choosing],
  ["書き込み中", writing],
] as const;

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

  it.each(inProgress)("%sは変わらない(書き出すパスフレーズを途中で変えない)", (_label, state) => {
    expect(regeneratePassphrase(state(), NEXT_PASSPHRASE)).toEqual(state());
  });

  it("成功後は変わらない(書き出したファイルと画面のパスフレーズを食い違わせない)", () => {
    expect(regeneratePassphrase(exported(), NEXT_PASSPHRASE)).toEqual(exported());
  });
});

describe("beginExport", () => {
  it("編集中で、開いた回の番号とパスフレーズが一致すれば、保存先の選択中になる(まだ何も書き出していない)", () => {
    expect(beginExport(editing(2), 2, PASSPHRASE)).toEqual(choosing(2));
  });

  it("開いた回の番号が異なれば変わらない(別の画面の状態を実行中にしない)", () => {
    expect(beginExport(editing(2), 1, PASSPHRASE)).toEqual(editing(2));
  });

  it("パスフレーズが異なれば変わらない(画面に出ているものと違うパスフレーズで書き出させない)", () => {
    expect(beginExport(editing(2), 2, NEXT_PASSPHRASE)).toEqual(editing(2));
  });

  it.each(inProgress)("%sは変わらない(二重実行を防ぐ)", (_label, state) => {
    expect(beginExport(state(), 1, PASSPHRASE)).toEqual(state());
  });

  it("成功後は変わらない(書き出し済みのパスフレーズで再実行させない)", () => {
    expect(beginExport(exported(), 1, PASSPHRASE)).toEqual(exported());
  });
});

describe("beginWriting", () => {
  it("保存先の選択中で、開いた回の番号が一致すれば、書き込み中になる(パスフレーズは保たれる)", () => {
    expect(beginWriting(choosing(2), 2)).toEqual(writing(2));
  });

  it("開いた回の番号が異なれば変わらない(閉じて開き直した後に届いた、古い選択の結果で、書き込まない)", () => {
    expect(beginWriting(choosing(2), 1)).toEqual(choosing(2));
  });

  it("保存先の選択中でなければ変わらない(閉じた画面のパスフレーズで、書き込みを始めない)", () => {
    expect(beginWriting(editing(2), 2)).toEqual(editing(2));
    expect(beginWriting(writing(2), 2)).toEqual(writing(2));
    expect(beginWriting(exported(2), 2)).toEqual(exported(2));
  });
});

describe("completeExport", () => {
  it("書き込み中で開いた回の番号が一致すれば、成功後になる(パスフレーズは保たれる)", () => {
    expect(completeExport(writing(2), 2)).toEqual(exported(2));
  });

  it("開いた回の番号が異なれば変わらない(閉じて開き直した後に届いた、古い実行の完了通知)", () => {
    expect(completeExport(writing(2), 1)).toEqual(writing(2));
  });

  it("書き込み中でなければ変わらない(保存先を選んでいる間に、成功にしない)", () => {
    expect(completeExport(editing(2), 2)).toEqual(editing(2));
    expect(completeExport(choosing(2), 2)).toEqual(choosing(2));
    expect(completeExport(exported(2), 2)).toEqual(exported(2));
  });
});

describe("abortExport", () => {
  it.each(inProgress)("%sで開いた回の番号が一致すれば、編集中へ戻る(取り消し・失敗)", (_label, state) => {
    expect(abortExport(state(2), 2)).toEqual(editing(2));
  });

  it.each(inProgress)("%sでも、開いた回の番号が異なれば変わらない", (_label, state) => {
    expect(abortExport(state(2), 1)).toEqual(state(2));
  });

  it("成功後は編集中へ戻らない(書き出し済みのファイルと画面のパスフレーズの対応を保つ)", () => {
    expect(abortExport(exported(2), 2)).toEqual(exported(2));
  });
});

describe("isPassphraseAtRisk", () => {
  it("編集中は、まだ何も書き出していないので、失っても失うものがない", () => {
    expect(isPassphraseAtRisk("editing")).toBe(false);
  });

  it("保存先の選択中も、まだ何も書き出していないので、失っても失うものがない(閉じられる)", () => {
    expect(isPassphraseAtRisk("choosing")).toBe(false);
  });

  it("書き込み中と成功後は、書き出したファイルを開ける唯一の手がかりなので、失わせてはならない", () => {
    expect(isPassphraseAtRisk("writing")).toBe(true);
    expect(isPassphraseAtRisk("exported")).toBe(true);
  });
});

describe("操作の可否", () => {
  it("再生成とエクスポートは、編集中だけ可能", () => {
    expect(canRegenerate(editing())).toBe(true);
    expect(canStartExport(editing())).toBe(true);
    for (const state of [choosing(), writing(), exported()]) {
      expect(canRegenerate(state)).toBe(false);
      expect(canStartExport(state)).toBe(false);
    }
  });
});
