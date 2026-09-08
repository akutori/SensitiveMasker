import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import { FileImportChoiceDialog } from "./file-import-choice-dialog";

const meta = {
  component: FileImportChoiceDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof FileImportChoiceDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const FILE_PATH = "C:\\Users\\example_user\\logs\\debug_console_output.log";
const LONG_FILE_PATH =
  "C:\\Users\\example_user\\Documents\\Projects\\some-very-long-project-name\\logs\\archive\\2026\\09\\debug_console_output_with_a_very_long_filename.log";

function DemoTrigger(props: { filePath: string; label: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="outline" onClick={() => setOpen(true)}>
        {props.label}
      </Button>
      <FileImportChoiceDialog
        open={open}
        onOpenChange={setOpen}
        filePath={props.filePath}
        onLoadIntoInput={() => {
          console.log("load into input");
          setOpen(false);
        }}
        onMaskAndSaveAs={() => {
          console.log("mask and save as");
          setOpen(false);
        }}
      />
    </>
  );
}

export const Default: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    filePath: FILE_PATH,
    onLoadIntoInput: () => {},
    onMaskAndSaveAs: () => {},
  },
  render: () => <DemoTrigger filePath={FILE_PATH} label="ファイルを開く" />,
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    filePath: FILE_PATH,
    onLoadIntoInput: () => {},
    onMaskAndSaveAs: () => {},
  },
};

export const LongFilePath: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    filePath: LONG_FILE_PATH,
    onLoadIntoInput: () => {},
    onMaskAndSaveAs: () => {},
  },
};
