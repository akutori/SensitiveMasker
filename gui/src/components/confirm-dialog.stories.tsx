import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "./confirm-dialog";

const meta = {
  component: ConfirmDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ConfirmDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

function DemoTrigger(props: {
  title: string;
  description: string;
  label: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="outline" onClick={() => setOpen(true)}>
        {props.label}
      </Button>
      <ConfirmDialog
        open={open}
        onOpenChange={setOpen}
        title={props.title}
        description={props.description}
        onConfirm={() => console.log(`confirmed: ${props.title}`)}
      />
    </>
  );
}

export const OverwriteInput: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    onConfirm: () => {},
    title: "上書き確認",
    description:
      "入力テキスト欄に入力済みの内容があります。ファイルの内容で上書きしますか?",
  },
  render: () => (
    <DemoTrigger
      title="上書き確認"
      description="入力テキスト欄に入力済みの内容があります。ファイルの内容で上書きしますか?"
      label="ファイルを読み込む"
    />
  ),
};

export const UnsavedRuleChanges: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    onConfirm: () => {},
    title: "変更の破棄確認",
    description: "編集中の内容が保存されていません。破棄してもよろしいですか?",
  },
  render: () => (
    <DemoTrigger
      title="変更の破棄確認"
      description="編集中の内容が保存されていません。破棄してもよろしいですか?"
      label="キャンセル"
    />
  ),
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    onConfirm: () => {},
    title: "上書き確認",
    description:
      "入力テキスト欄に入力済みの内容があります。ファイルの内容で上書きしますか?",
  },
};
