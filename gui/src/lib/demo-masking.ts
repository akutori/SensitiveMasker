import type { RuleListItem } from "@/components/rule-edit-screen";

export interface MaskResult {
  text: string;
  matchCounts: Record<string, number>;
}

export function simulateMask(text: string, rules: RuleListItem[]): MaskResult {
  let result = text;
  const counters: Record<string, number> = {};
  const matchCounts: Record<string, number> = {};
  for (const rule of rules) {
    matchCounts[rule.id] = 0;
    if (!rule.enabled) continue;
    try {
      let source =
        rule.patternType === "regex"
          ? rule.pattern
          : rule.pattern.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      // Rust(regex crate)由来のパターンが(?i)インラインフラグを使うことがあるため、
      // JSのRegExpが解釈できるよう`i`フラグに変換する。
      let flags = "g";
      if (source.startsWith("(?i)")) {
        source = source.slice(4);
        flags += "i";
      }
      const regex = new RegExp(source, flags);
      result = result.replace(regex, () => {
        matchCounts[rule.id] += 1;
        if (rule.mode === "fixed") return rule.fixedValue || "";
        counters[rule.id] = (counters[rule.id] ?? 0) + 1;
        return `${rule.prefix}${counters[rule.id]}`;
      });
    } catch {
      // 無効な正規表現はデモ上スキップ(実際の検証はRuleEditDialog側のerrorMessageで行う)
    }
  }
  return { text: result, matchCounts };
}
