import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import {
  MatchCountConfirmDialog,
  type MatchCountRow,
} from "./match-count-confirm-dialog";

const meta = {
  component: MatchCountConfirmDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof MatchCountConfirmDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const ROWS_WITH_ZERO: MatchCountRow[] = [
  { ruleName: "電話番号", matchCount: 12 },
  { ruleName: "メールアドレス", matchCount: 5 },
  { ruleName: "IPアドレス", matchCount: 0 },
  { ruleName: "クレジットカード番号", matchCount: 3 },
  { ruleName: "パスワード", matchCount: 8 },
];

const ROWS_ALL_MATCHED: MatchCountRow[] = [
  { ruleName: "電話番号", matchCount: 12 },
  { ruleName: "メールアドレス", matchCount: 5 },
];

function DemoTrigger(props: { rows: MatchCountRow[]; label: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="outline" onClick={() => setOpen(true)}>
        {props.label}
      </Button>
      <MatchCountConfirmDialog
        open={open}
        onOpenChange={setOpen}
        rows={props.rows}
        onConfirm={() => console.log("confirmed with rows:", props.rows)}
      />
    </>
  );
}

export const WithZeroMatchWarning: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    onConfirm: () => {},
    rows: ROWS_WITH_ZERO,
  },
  render: () => (
    <DemoTrigger rows={ROWS_WITH_ZERO} label="直接変換して保存" />
  ),
};

export const AllRulesMatched: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    onConfirm: () => {},
    rows: ROWS_ALL_MATCHED,
  },
  render: () => (
    <DemoTrigger rows={ROWS_ALL_MATCHED} label="直接変換して保存" />
  ),
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    onConfirm: () => {},
    rows: ROWS_WITH_ZERO,
  },
};
