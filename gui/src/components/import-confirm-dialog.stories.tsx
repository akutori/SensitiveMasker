import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
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

const MULTIPLE_ROWS: ImportPreviewRow[] = [
  { profileName: "work", result: "そのまま作成" },
  { profileName: "personal", result: "そのまま作成" },
  {
    profileName: "SIP監視用",
    result: "'SIP監視用 (インポート)' としてリネーム(重複のため)",
  },
];

const SINGLE_ROW: ImportPreviewRow[] = [
  { profileName: "検証用サンプル", result: "そのまま作成" },
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
