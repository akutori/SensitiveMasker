import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import { EnvImportSelectDialog } from "./env-import-select-dialog";
import type { EnvCandidate } from "@/lib/env-import-ipc";

const meta = {
  component: EnvImportSelectDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof EnvImportSelectDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const SAMPLE_CANDIDATES: EnvCandidate[] = [
  {
    key: "DATABASE_URL",
    value: "postgres://dummyuser:dummyCorrectHorseBattery42@db.example.internal:5432/appdb",
    includedByDefault: true,
  },
  { key: "API_KEY", value: "sk_dummy_51H8xNOTREALxJf92kQpLmZ", includedByDefault: true },
  { key: "JWT_SECRET", value: "dummy-jwt-secret-9f8e7d6c5b4a", includedByDefault: true },
  { key: "PORT", value: "3000", includedByDefault: false },
  { key: "DEBUG", value: "true", includedByDefault: false },
  { key: "NODE_ENV", value: "production", includedByDefault: false },
  { key: "LOG_LEVEL", value: "info", includedByDefault: false },
  { key: "SHORT", value: "ab", includedByDefault: false },
];

function DemoTrigger() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="outline" onClick={() => setOpen(true)}>
        envインポート
      </Button>
      <EnvImportSelectDialog
        open={open}
        onOpenChange={setOpen}
        candidates={SAMPLE_CANDIDATES}
        onConfirm={() => setOpen(false)}
      />
    </>
  );
}

export const Default: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    candidates: SAMPLE_CANDIDATES,
    onConfirm: () => {},
  },
  render: () => <DemoTrigger />,
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    candidates: SAMPLE_CANDIDATES,
    onConfirm: () => {},
  },
};

export const NoImportableItems: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    candidates: [],
    onConfirm: () => {},
  },
};
