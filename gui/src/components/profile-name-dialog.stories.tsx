import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import { ProfileNameDialog } from "./profile-name-dialog";

const meta = {
  component: ProfileNameDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ProfileNameDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const DUPLICATE_NAME_ERROR = "同じ名前のプロファイルが既に存在します";

function DemoTrigger(props: { initialName: string; label: string }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(props.initialName);
  const [errorMessage, setErrorMessage] = useState<string | undefined>();

  return (
    <>
      <Button
        variant="outline"
        onClick={() => {
          setName(props.initialName);
          setErrorMessage(undefined);
          setOpen(true);
        }}
      >
        {props.label}
      </Button>
      <ProfileNameDialog
        open={open}
        onOpenChange={setOpen}
        name={name}
        onNameChange={(value) => {
          setName(value);
          setErrorMessage(undefined);
        }}
        errorMessage={errorMessage}
        onConfirm={() => {
          if (name === "SIP監視用") {
            setErrorMessage(DUPLICATE_NAME_ERROR);
          } else {
            setOpen(false);
          }
        }}
      />
    </>
  );
}

export const NewProfile: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    name: "新しいプロファイル",
    onNameChange: () => {},
    onConfirm: () => {},
  },
  render: () => (
    <DemoTrigger initialName="新しいプロファイル" label="新規プロファイル" />
  ),
};

export const FromTemplate: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    name: "SIP監視用",
    onNameChange: () => {},
    onConfirm: () => {},
  },
  render: () => (
    <DemoTrigger initialName="SIP監視用" label="テンプレートから作成(重複例)" />
  ),
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    name: "新しいプロファイル",
    onNameChange: () => {},
    onConfirm: () => {},
  },
};

export const WithDuplicateNameError: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    name: "SIP監視用",
    onNameChange: () => {},
    errorMessage: DUPLICATE_NAME_ERROR,
    onConfirm: () => {},
  },
};
