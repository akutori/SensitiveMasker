import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import { ExportModal } from "./export-modal";

const meta = {
  component: ExportModal,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ExportModal>;

export default meta;
type Story = StoryObj<typeof meta>;

const INITIAL_PASSPHRASE = "K7m-9pQx-2Lw4-Rt8v";

function randomDemoPassphrase() {
  const words = Array.from({ length: 4 }, () =>
    Math.random().toString(36).slice(2, 6)
  );
  return words.join("-");
}

function DemoTrigger(props: { target: string; label: string }) {
  const [open, setOpen] = useState(false);
  const [passphrase, setPassphrase] = useState(INITIAL_PASSPHRASE);

  return (
    <>
      <Button
        variant="outline"
        onClick={() => {
          setPassphrase(INITIAL_PASSPHRASE);
          setOpen(true);
        }}
      >
        {props.label}
      </Button>
      <ExportModal
        open={open}
        onOpenChange={setOpen}
        target={props.target}
        passphrase={passphrase}
        busy={false}
        onCopy={() => {
          navigator.clipboard.writeText(passphrase);
        }}
        onRegenerate={() => setPassphrase(randomDemoPassphrase())}
        onExport={() => setOpen(false)}
      />
    </>
  );
}

export const SingleProfile: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    busy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  render: () => <DemoTrigger target="SIP監視用" label="エクスポート(単一)" />,
};

export const AllProfiles: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    target: "全プロファイル",
    passphrase: INITIAL_PASSPHRASE,
    busy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
  render: () => <DemoTrigger target="全プロファイル" label="エクスポート(全体)" />,
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    busy: false,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
};

// コピー/クリアのIPC応答待ちの間、コピー・再生成ボタンが非活性化されることの確認用。
export const BusyDisablesButtons: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    target: "SIP監視用",
    passphrase: INITIAL_PASSPHRASE,
    busy: true,
    onCopy: () => {},
    onRegenerate: () => {},
    onExport: () => {},
  },
};
