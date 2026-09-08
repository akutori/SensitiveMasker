import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import {
  RuleEditDialog,
  type RuleFormValues,
  type RuleTemplateOption,
} from "./rule-edit-dialog";

const meta = {
  component: RuleEditDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof RuleEditDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const EMPTY_VALUES: RuleFormValues = {
  name: "",
  patternType: "literal",
  pattern: "",
  mode: "fixed",
  fixedValue: "",
  prefix: "",
  enabled: true,
  description: "",
};

const PHONE_RULE_VALUES: RuleFormValues = {
  name: "電話番号",
  patternType: "regex",
  pattern: "0\\d{1,4}-?\\d{1,4}-?\\d{3,4}",
  mode: "sequential",
  fixedValue: "",
  prefix: "__MASK_TEL_",
  enabled: true,
  description: "国内電話番号(ハイフン有無どちらも許容)",
};

const EMAIL_RULE_VALUES: RuleFormValues = {
  name: "メールアドレス",
  patternType: "regex",
  pattern: "[\\w.+-]+@[\\w-]+\\.[\\w.-]+",
  mode: "sequential",
  fixedValue: "",
  prefix: "__MASK_EMAIL_",
  enabled: true,
  description: "一般的なメールアドレス形式",
};

const IP_RULE_VALUES: RuleFormValues = {
  name: "IPアドレス",
  patternType: "regex",
  pattern: "\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}\\.\\d{1,3}",
  mode: "sequential",
  fixedValue: "",
  prefix: "__MASK_IP_",
  enabled: true,
  description: "IPv4アドレス",
};

const TEMPLATE_VALUES_BY_ID: Record<string, RuleFormValues> = {
  phone: PHONE_RULE_VALUES,
  email: EMAIL_RULE_VALUES,
  ip: IP_RULE_VALUES,
};

const FIXED_MODE_VALUES: RuleFormValues = {
  ...PHONE_RULE_VALUES,
  mode: "fixed",
  fixedValue: "[REDACTED]",
  prefix: "",
};

const TEMPLATE_OPTIONS: RuleTemplateOption[] = [
  { value: "phone", label: "電話番号" },
  { value: "email", label: "メールアドレス" },
  { value: "ip", label: "IPアドレス" },
];

function DemoTrigger(props: { initialValues: RuleFormValues; label: string }) {
  const [open, setOpen] = useState(false);
  const [values, setValues] = useState(props.initialValues);
  const [errorMessage, setErrorMessage] = useState<string | undefined>();
  const [invalidField, setInvalidField] = useState<keyof RuleFormValues | undefined>();

  const updateValues = (next: RuleFormValues) => {
    setValues(next);
    setErrorMessage(undefined);
    setInvalidField(undefined);
  };

  return (
    <>
      <Button
        variant="outline"
        onClick={() => {
          setValues(props.initialValues);
          setErrorMessage(undefined);
          setInvalidField(undefined);
          setOpen(true);
        }}
      >
        {props.label}
      </Button>
      <RuleEditDialog
        open={open}
        onOpenChange={setOpen}
        values={values}
        onValuesChange={updateValues}
        templateOptions={TEMPLATE_OPTIONS}
        onTemplateSelect={(templateValue) => {
          const templateValues = TEMPLATE_VALUES_BY_ID[templateValue];
          if (templateValues) updateValues(templateValues);
        }}
        errorMessage={errorMessage}
        invalidField={invalidField}
        onConfirm={() => {
          if (values.patternType === "regex" && values.pattern === "(") {
            setErrorMessage("入力エラー: パターンが正しい正規表現ではありません");
            setInvalidField("pattern");
          } else {
            setOpen(false);
          }
        }}
      />
    </>
  );
}

export const NewRule: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    values: EMPTY_VALUES,
    onValuesChange: () => {},
    templateOptions: TEMPLATE_OPTIONS,
    onTemplateSelect: () => {},
    onConfirm: () => {},
  },
  render: () => <DemoTrigger initialValues={EMPTY_VALUES} label="ルールを追加" />,
};

export const EditingSequentialMode: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    values: PHONE_RULE_VALUES,
    onValuesChange: () => {},
    templateOptions: TEMPLATE_OPTIONS,
    onTemplateSelect: () => {},
    onConfirm: () => {},
  },
};

export const EditingFixedMode: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    values: FIXED_MODE_VALUES,
    onValuesChange: () => {},
    templateOptions: TEMPLATE_OPTIONS,
    onTemplateSelect: () => {},
    onConfirm: () => {},
  },
};

export const WithValidationError: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    values: { ...PHONE_RULE_VALUES, pattern: "(" },
    onValuesChange: () => {},
    templateOptions: TEMPLATE_OPTIONS,
    onTemplateSelect: () => {},
    errorMessage: "入力エラー: パターンが正しい正規表現ではありません",
    invalidField: "pattern",
    onConfirm: () => {},
  },
};
