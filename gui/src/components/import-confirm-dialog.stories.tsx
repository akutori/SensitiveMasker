import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, userEvent } from "storybook/test";
import { Button } from "@/components/ui/button";
import type { ImportRuleDto } from "@/lib/profile-ipc";
import {
  ImportConfirmDialog,
  type ImportPreviewRow,
} from "./import-confirm-dialog";

const meta = {
  component: ImportConfirmDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ImportConfirmDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const WORK_RULES: ImportRuleDto[] = [
  {
    name: "内線番号",
    pattern_type: "literal",
    pattern: "0120",
    mode: "sequential",
    fixed_value: null,
    prefix: "EXT",
    enabled: true,
  },
  {
    name: "社内IPアドレス",
    pattern_type: "regex",
    pattern: "203\\.0\\.113\\.\\d{1,3}",
    mode: "sequential",
    fixed_value: null,
    prefix: "IP",
    enabled: true,
  },
];

const PERSONAL_RULES: ImportRuleDto[] = [
  {
    name: "メールアドレス",
    pattern_type: "regex",
    pattern: "[\\w.+-]+@example\\.com",
    mode: "fixed",
    fixed_value: "[MASKED_EMAIL]",
    prefix: null,
    enabled: true,
  },
];

// 無効ルールが薄く表示されることを確認するためのサンプル(旧パスワードルール)。
const SIP_RULES: ImportRuleDto[] = [
  {
    name: "SIP URI",
    pattern_type: "regex",
    pattern: "sip:[\\w.]+@203\\.0\\.113\\.\\d{1,3}",
    mode: "sequential",
    fixed_value: null,
    prefix: "SIP",
    enabled: true,
  },
  {
    name: "旧パスワードルール",
    pattern_type: "literal",
    pattern: "hunter2",
    mode: "fixed",
    fixed_value: "[PASSWORD]",
    prefix: null,
    enabled: false,
  },
];

const SAMPLE_RULES: ImportRuleDto[] = [
  {
    name: "電話番号",
    pattern_type: "literal",
    pattern: "0120",
    mode: "sequential",
    fixed_value: null,
    prefix: "TEL",
    enabled: true,
  },
];

const MULTIPLE_ROWS: ImportPreviewRow[] = [
  { profileName: "work", result: "そのまま作成", rules: WORK_RULES },
  { profileName: "personal", result: "そのまま作成", rules: PERSONAL_RULES },
  {
    profileName: "SIP監視用",
    result: "'SIP監視用 (インポート)' としてリネーム(重複のため)",
    rules: SIP_RULES,
  },
];

const SINGLE_ROW: ImportPreviewRow[] = [
  { profileName: "検証用サンプル", result: "そのまま作成", rules: SAMPLE_RULES },
];

function DemoTrigger(props: { rows: ImportPreviewRow[]; label: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="outline" onClick={() => setOpen(true)}>
        {props.label}
      </Button>
      <ImportConfirmDialog
        open={open}
        onOpenChange={setOpen}
        rows={props.rows}
        onConfirm={() => console.log("import confirmed:", props.rows)}
      />
    </>
  );
}

export const MultipleProfiles: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    onConfirm: () => {},
    rows: MULTIPLE_ROWS,
  },
  render: () => <DemoTrigger rows={MULTIPLE_ROWS} label="インポート(全体)" />,
};

export const SingleProfile: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    onConfirm: () => {},
    rows: SINGLE_ROW,
  },
  render: () => <DemoTrigger rows={SINGLE_ROW} label="インポート(単一)" />,
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    onConfirm: () => {},
    rows: MULTIPLE_ROWS,
  },
};

// 呼び出された順序(vitestのモックが、呼び出しごとに振る通し番号)。
function callOrder(spy: unknown): number {
  return (spy as { mock: { invocationCallOrder: number[] } }).mock.invocationCallOrder[0];
}

// 「インポート実行」は、確定(onConfirm)を先に呼び、その後で画面を閉じる操作(onOpenChange(false))を
// 呼ぶ。呼び出し側は、この順序に頼って、確定を始めたことを閉じる操作より前に記録し、確定と、閉じる操作に
// 伴う後始末を続けて発行しない(順序が保証されず、後始末が先に走ると、確定が失敗するため)。
export const ConfirmBeforeClosing: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    onConfirm: fn(),
    rows: SINGLE_ROW,
  },
  play: async ({ args }) => {
    await userEvent.click(await screen.findByRole("button", { name: "インポート実行" }));
    await expect(args.onConfirm).toHaveBeenCalledTimes(1);
    await expect(args.onOpenChange).toHaveBeenCalledWith(false);
    await expect(callOrder(args.onConfirm)).toBeLessThan(callOrder(args.onOpenChange));
  },
};

// キャンセルは、確定を呼ばずに、画面を閉じる操作だけを呼ぶ(呼び出し側は、これを確定しない閉じ方と
// みなして、復号済みの内容を破棄する)。
export const CancelDoesNotConfirm: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    onConfirm: fn(),
    rows: SINGLE_ROW,
  },
  play: async ({ args }) => {
    await userEvent.click(await screen.findByRole("button", { name: "キャンセル" }));
    await expect(args.onOpenChange).toHaveBeenCalledWith(false);
    await expect(args.onConfirm).not.toHaveBeenCalled();
  },
};
