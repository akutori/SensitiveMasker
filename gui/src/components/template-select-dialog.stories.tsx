import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import { DEFAULT_TEMPLATES, TemplateSelectDialog } from "./template-select-dialog";

const meta = {
  component: TemplateSelectDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof TemplateSelectDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

function DemoTrigger() {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("general");

  return (
    <>
      <Button variant="outline" onClick={() => setOpen(true)}>
        テンプレートから作成
      </Button>
      <TemplateSelectDialog
        open={open}
        onOpenChange={setOpen}
        templates={DEFAULT_TEMPLATES}
        value={value}
        onValueChange={setValue}
        onConfirm={() => {
          console.log("selected template:", value);
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
    templates: DEFAULT_TEMPLATES,
    value: "general",
    onValueChange: () => {},
    onConfirm: () => {},
  },
  render: () => <DemoTrigger />,
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    templates: DEFAULT_TEMPLATES,
    value: "general",
    onValueChange: () => {},
    onConfirm: () => {},
  },
};

export const SipSelected: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    templates: DEFAULT_TEMPLATES,
    value: "sip",
    onValueChange: () => {},
    onConfirm: () => {},
  },
};
