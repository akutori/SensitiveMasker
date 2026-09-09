import { invoke } from "@tauri-apps/api/core";
import type { RuleListItem } from "@/components/rule-edit-screen";

export interface RuleDto {
  name: string;
  pattern_type: "literal" | "regex";
  pattern: string;
  mode: "fixed" | "sequential";
  fixed_value: string | null;
  prefix: string | null;
  enabled: boolean;
  description: string | null;
}

export interface RuleProfileDto {
  profile_name: string;
  description: string | null;
  rules: RuleDto[];
}

interface MatchCountDto {
  rule_name: string;
  count: number;
}

interface MaskTextResponse {
  text: string;
  match_counts: MatchCountDto[];
}

export interface MaskMatchCount {
  ruleName: string;
  count: number;
}

export interface MaskTextResult {
  text: string;
  matchCounts: MaskMatchCount[];
}

export function toRuleDto(rule: RuleListItem): RuleDto {
  return {
    name: rule.name,
    pattern_type: rule.patternType,
    pattern: rule.pattern,
    mode: rule.mode,
    fixed_value: rule.fixedValue || null,
    prefix: rule.prefix || null,
    enabled: rule.enabled,
    description: rule.description || null,
  };
}

// masking-coreはGUI固有のid概念を持たないため、Rust側から返るルールには無く、
// ここでReact用の一時的なidを新規に振る。
export function fromRuleDto(dto: RuleDto): RuleListItem {
  return {
    id: crypto.randomUUID(),
    name: dto.name,
    patternType: dto.pattern_type,
    pattern: dto.pattern,
    mode: dto.mode,
    fixedValue: dto.fixed_value ?? "",
    prefix: dto.prefix ?? "",
    enabled: dto.enabled,
    description: dto.description ?? "",
  };
}

// profileIdはMappingStoreの永続化単位(masking-coreの連番モードは呼び出しをまたいで
// 同じ値に同じ番号を割り当てるため、Rust側でprofileIdごとに保持している)。
export async function maskText(
  profileId: string,
  profileName: string,
  rules: RuleListItem[],
  text: string
): Promise<MaskTextResult> {
  const profile: RuleProfileDto = {
    profile_name: profileName,
    description: null,
    rules: rules.map(toRuleDto),
  };
  const response = await invoke<MaskTextResponse>("mask_text", {
    profileId,
    profile,
    text,
  });
  return {
    text: response.text,
    matchCounts: response.match_counts.map((m) => ({
      ruleName: m.rule_name,
      count: m.count,
    })),
  };
}
