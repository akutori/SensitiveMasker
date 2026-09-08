import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
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
};
