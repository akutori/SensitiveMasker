import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, screen, userEvent, waitFor, within } from "storybook/test";
import { Button } from "@/components/ui/button";
import { ImportPassphraseDialog } from "./import-passphrase-dialog";

const meta = {
  component: ImportPassphraseDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ImportPassphraseDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const FILE_NAME = "profiles_export_20260901.smx";
const PASSPHRASE_LABEL = "パスフレーズ:";
const ERROR_MESSAGE =
  "パスフレーズが誤っているか、対応していないファイル形式です";

function DemoTrigger(props: { initialError?: string }) {
  const [open, setOpen] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const [errorMessage, setErrorMessage] = useState(props.initialError);

  return (
    <>
      <Button
        variant="outline"
        onClick={() => {
          setPassphrase("");
          setErrorMessage(props.initialError);
          setOpen(true);
        }}
      >
        インポート
      </Button>
      <ImportPassphraseDialog
        open={open}
        onOpenChange={setOpen}
        fileName={FILE_NAME}
        passphrase={passphrase}
        onPassphraseChange={(value) => {
          setPassphrase(value);
          setErrorMessage(undefined);
        }}
        errorMessage={errorMessage}
        onConfirm={() => {
          if (passphrase === "correct") {
            setOpen(false);
          } else {
            setErrorMessage(ERROR_MESSAGE);
          }
        }}
      />
    </>
  );
}

export const Default: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "",
    onPassphraseChange: () => {},
    onConfirm: () => {},
  },
  render: () => <DemoTrigger />,
};

export const WithError: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "",
    onPassphraseChange: () => {},
    errorMessage: ERROR_MESSAGE,
    onConfirm: () => {},
  },
  render: () => <DemoTrigger initialError={ERROR_MESSAGE} />,
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "wrong-passphrase",
    onPassphraseChange: () => {},
    errorMessage: ERROR_MESSAGE,
    onConfirm: () => {},
  },
  // 入力欄は伏せ字から始まり、表示切替で入力内容を確認できる(貼り付けに混ざった空白などを
  // 目で確かめられるようにするため)。
  play: async () => {
    const input = await screen.findByLabelText(PASSPHRASE_LABEL);
    await expect(input).toHaveAttribute("type", "password");

    await userEvent.click(screen.getByRole("button", { name: "パスフレーズを表示" }));
    await expect(input).toHaveAttribute("type", "text");
    await expect(input).toHaveValue("wrong-passphrase");

    await userEvent.click(screen.getByRole("button", { name: "パスフレーズを隠す" }));
    await expect(input).toHaveAttribute("type", "password");
  },
};

// 開き直すたびに、表示していても伏せ字から始まる(前回の表示状態を引き継がない)。
export const RevealResetsWhenReopened: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    fileName: FILE_NAME,
    passphrase: "",
    onPassphraseChange: () => {},
    onConfirm: () => {},
  },
  render: () => <DemoTrigger />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(await canvas.findByRole("button", { name: "インポート" }));
    const input = await screen.findByLabelText(PASSPHRASE_LABEL);
    await userEvent.click(screen.getByRole("button", { name: "パスフレーズを表示" }));
    await expect(input).toHaveAttribute("type", "text");

    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    await userEvent.click(await canvas.findByRole("button", { name: "インポート" }));
    await expect(await screen.findByLabelText(PASSPHRASE_LABEL)).toHaveAttribute("type", "password");
  },
};
