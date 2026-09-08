import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import { DeleteConfirmDialog } from "./delete-confirm-dialog";

const meta = {
  component: DeleteConfirmDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof DeleteConfirmDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

function DemoTrigger(props: {
  targetType: string;
  targetName: string;
  label: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="outline" onClick={() => setOpen(true)}>
        {props.label}
      </Button>
      <DeleteConfirmDialog
        open={open}
        onOpenChange={setOpen}
        targetType={props.targetType}
        targetName={props.targetName}
        onConfirm={() => console.log(`${props.targetType} "${props.targetName}" deleted`)}
      />
    </>
  );
}

export const RuleDeletion: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    onConfirm: () => {},
    targetType: "ルール",
    targetName: "電話番号",
  },
  render: () => (
    <DemoTrigger targetType="ルール" targetName="電話番号" label="ルールを削除" />
  ),
};

export const ProfileDeletion: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    onConfirm: () => {},
    targetType: "プロファイル",
    targetName: "検証用サンプル",
  },
  render: () => (
    <DemoTrigger
      targetType="プロファイル"
      targetName="検証用サンプル"
      label="プロファイルを削除"
    />
  ),
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    onConfirm: () => {},
    targetType: "タグ",
    targetName: "SIP",
  },
};
