import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  RuleEditScreen,
  type RuleListItem,
} from "./rule-edit-screen";
import type { RuleFormValues, RuleTemplateOption } from "./rule-edit-dialog";

const meta = {
  component: RuleEditScreen,
  parameters: {
    layout: "fullscreen",
  },
} satisfies Meta<typeof RuleEditScreen>;

export default meta;
type Story = StoryObj<typeof meta>;

const INITIAL_RULES: RuleListItem[] = [
  {
    id: "1",
    name: "電話番号",
    patternType: "regex",
    pattern: "0\\d{2,4}-\\d{2,4}-\\d{3,4}",
    mode: "sequential",
    fixedValue: "",
    prefix: "__MASK_TEL_",
    enabled: true,
    description: "電話番号",
  },
  {
    id: "2",
    name: "SIP URI",
    patternType: "regex",
    pattern: "sip:[\\w.]+@[\\d.]+",
    mode: "sequential",
    fixedValue: "",
    prefix: "__MASK_SIP_",
    enabled: true,
    description: "SIP URI",
  },
  {
    id: "3",
    name: "パスワード",
    patternType: "literal",
    pattern: "hunter2",
    mode: "fixed",
    fixedValue: "[REDACTED]",
    prefix: "",
    enabled: false,
    description: "パスワード",
  },
];

const SAMPLE_TEXT =
  "着信: 0120-000-000\nSIP URI: sip:alice@203.0.113.10\nパスワード: hunter2";

const RULE_TEMPLATE_OPTIONS: RuleTemplateOption[] = [
  { value: "phone", label: "電話番号" },
  { value: "email", label: "メールアドレス" },
  { value: "ip", label: "IPアドレス" },
];

const RULE_TEMPLATE_VALUES: Record<string, Partial<RuleFormValues>> = {
  phone: {
    name: "電話番号",
    patternType: "regex",
    pattern: "0\\d{2,4}-\\d{2,4}-\\d{3,4}",
    mode: "sequential",
    prefix: "__MASK_TEL_",
    description: "電話番号",
  },
  email: {
    name: "メールアドレス",
    patternType: "regex",
    pattern: "[\\w.+-]+@[\\w-]+\\.[\\w.-]+",
    mode: "sequential",
    prefix: "__MASK_EMAIL_",
    description: "メールアドレス",
  },
  ip: {
    name: "IPアドレス",
    patternType: "regex",
    pattern: "\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}",
    mode: "sequential",
    prefix: "__MASK_IP_",
    description: "IPアドレス",
  },
};

function computeMaskedResult(text: string, rules: RuleListItem[]): string {
  let result = text;
  const counters: Record<string, number> = {};
  for (const rule of rules) {
    if (!rule.enabled) continue;
    try {
      const source =
        rule.patternType === "regex"
          ? rule.pattern
          : rule.pattern.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const regex = new RegExp(source, "g");
      result = result.replace(regex, () => {
        if (rule.mode === "fixed") return rule.fixedValue || "";
        counters[rule.id] = (counters[rule.id] ?? 0) + 1;
        return `${rule.prefix}${counters[rule.id]}`;
      });
    } catch {
      // 無効な正規表現はデモ上スキップ(実際の検証はRuleEditDialog側のerrorMessageで行う)
    }
  }
  return result;
}

function DemoScreen(props: { initialRules: RuleListItem[] }) {
  const [profileName, setProfileName] = useState("SIP監視用");
  const [profileDescription, setProfileDescription] = useState(
    "SIPサーバーのアクセスログ用"
  );
  const [rules, setRules] = useState(props.initialRules);
  const [sampleText, setSampleText] = useState(SAMPLE_TEXT);

  return (
    <RuleEditScreen
      profileName={profileName}
      onProfileNameChange={setProfileName}
      profileDescription={profileDescription}
      onProfileDescriptionChange={setProfileDescription}
      rules={rules}
      onReorderRules={setRules}
      onToggleRuleEnabled={(id) =>
        setRules(rules.map((r) => (r.id === id ? { ...r, enabled: !r.enabled } : r)))
      }
      onAddRule={(values) =>
        setRules([...rules, { ...values, id: crypto.randomUUID() }])
      }
      onEditRule={(id, values) =>
        setRules(rules.map((r) => (r.id === id ? { ...values, id } : r)))
      }
      onDeleteRule={(id) => setRules(rules.filter((r) => r.id !== id))}
      ruleTemplateOptions={RULE_TEMPLATE_OPTIONS}
      onResolveRuleTemplate={(value) => RULE_TEMPLATE_VALUES[value] ?? {}}
      sampleText={sampleText}
      onSampleTextChange={setSampleText}
      maskedResult={computeMaskedResult(sampleText, rules)}
      onSave={() => console.log("save")}
      onCancel={() => console.log("cancel")}
    />
  );
}

export const Default: Story = {
  args: {
    profileName: "SIP監視用",
    onProfileNameChange: () => {},
    profileDescription: "SIPサーバーのアクセスログ用",
    onProfileDescriptionChange: () => {},
    rules: INITIAL_RULES,
    onReorderRules: () => {},
    onToggleRuleEnabled: () => {},
    onAddRule: () => {},
    onEditRule: () => {},
    onDeleteRule: () => {},
    ruleTemplateOptions: RULE_TEMPLATE_OPTIONS,
    onResolveRuleTemplate: () => ({}),
    sampleText: SAMPLE_TEXT,
    onSampleTextChange: () => {},
    maskedResult: computeMaskedResult(SAMPLE_TEXT, INITIAL_RULES),
    onSave: () => {},
    onCancel: () => {},
  },
  render: () => <DemoScreen initialRules={INITIAL_RULES} />,
};

export const EmptyRules: Story = {
  args: {
    ...Default.args,
    rules: [],
    maskedResult: SAMPLE_TEXT,
  },
  render: () => <DemoScreen initialRules={[]} />,
};
