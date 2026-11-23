// マスク実行でマッチ件数が0件だったルールを、トーストで知らせる。
import { toast } from "sonner";
import type { MaskMatchCount } from "./masking-ipc";

// 文言が長く、読み終えるまでの時間が要るため、既定(4秒)より少し長くしている。
export const ZERO_MATCH_TOAST_DURATION_MS = 5000;

export function zeroMatchRulesMessage(ruleNames: string[]): string {
  return `マッチ件数が0件のルールがあります(${ruleNames.join("、")})。意図した設定でない場合はルール設定を見直してください。`;
}

// 有効なルールはマッチ0件でもmatchCountsに残る(無効ルールのみ含まれない)。意図しない正規表現ミス等に
// 気付けるよう、実行のたびに知らせる。
//
// 左下に出すのは、既定の右下だと、右寄せの「ファイルに保存」「クリップボードにコピー」を覆い、
// カーソルがトーストの上にある間は自動で閉じるタイマーも止まって、ボタンを押せなくなるため。
// 右上は、プロファイル選択行のボタンと重なる。
export function notifyZeroMatchRules(matchCounts: Pick<MaskMatchCount, "ruleName" | "count">[]): void {
  const zeroMatchRules = matchCounts.filter((m) => m.count === 0);
  if (zeroMatchRules.length === 0) return;
  toast.warning(zeroMatchRulesMessage(zeroMatchRules.map((m) => m.ruleName)), {
    closeButton: true,
    duration: ZERO_MATCH_TOAST_DURATION_MS,
    position: "bottom-left",
  });
}
