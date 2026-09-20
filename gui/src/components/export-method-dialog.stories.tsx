import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, userEvent, waitFor } from "storybook/test";
import { ExportMethodDialog, type ExportMethod } from "./export-method-dialog";

const meta = {
  component: ExportMethodDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ExportMethodDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

function Demo(props: { onNext: () => void; initial?: ExportMethod }) {
  const [method, setMethod] = useState<ExportMethod>(props.initial ?? "key_file");
  return (
    <ExportMethodDialog
      open
      onOpenChange={() => {}}
      target="SIP監視用"
      method={method}
      onMethodChange={setMethod}
      onNext={props.onNext}
    />
  );
}

// 鍵ファイルが上に置かれ、初期状態で選択されている。
export const KeyFileIsSelectedByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    target: "SIP監視用",
    method: "key_file",
    onMethodChange: () => {},
    onNext: () => {},
  },
  render: (args) => <Demo onNext={args.onNext} />,
  play: async () => {
    const options = await screen.findAllByRole("radio");
    await expect(options).toHaveLength(2);
    await expect(options[0]).toHaveAccessibleName(/鍵ファイル\(推奨\)/);
    await expect(options[0]).toBeChecked();
    await expect(options[1]).toHaveAccessibleName(/パスフレーズ/);
    await expect(options[1]).not.toBeChecked();
    await waitFor(() => expect(screen.getByText("復号キーを使用してインポートします")).toBeVisible());
    await waitFor(() => expect(screen.getByText("生成されたパスフレーズを使ってインポートします")).toBeVisible());
  },
};

// パスフレーズを選ぶと、選択が移り、次へ進める。
export const ChoosePassphraseThenNext: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    target: "全プロファイル",
    method: "key_file",
    onMethodChange: () => {},
    onNext: fn(),
  },
  render: (args) => <Demo onNext={args.onNext} />,
  play: async ({ args }) => {
    const [keyFile, passphrase] = await screen.findAllByRole("radio");

    await userEvent.click(passphrase);
    await expect(passphrase).toBeChecked();
    await expect(keyFile).not.toBeChecked();

    await userEvent.click(screen.getByRole("button", { name: "次へ" }));
    await expect(args.onNext).toHaveBeenCalledTimes(1);
  },
};

// 選択の文言(カードの全体)を押しても、選べる。
export const ClickingTheCardTextSelects: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    target: "SIP監視用",
    method: "key_file",
    onMethodChange: () => {},
    onNext: () => {},
  },
  render: (args) => <Demo onNext={args.onNext} />,
  play: async () => {
    await userEvent.click(await screen.findByText("生成されたパスフレーズを使ってインポートします"));

    const [keyFile, passphrase] = screen.getAllByRole("radio");
    await expect(passphrase).toBeChecked();
    await expect(keyFile).not.toBeChecked();
  },
};

export const CancelClosesTheDialog: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    target: "SIP監視用",
    method: "key_file",
    onMethodChange: () => {},
    onNext: () => {},
  },
  play: async ({ args }) => {
    await userEvent.click(await screen.findByRole("button", { name: "キャンセル" }));
    await expect(args.onOpenChange).toHaveBeenCalledWith(false);
  },
};
