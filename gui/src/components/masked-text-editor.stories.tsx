import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { MaskedTextEditor } from "./masked-text-editor";

const meta = {
  component: MaskedTextEditor,
  parameters: {
    layout: "centered",
  },
  decorators: [
    (Story) => (
      <div className="w-[860px]">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof MaskedTextEditor>;

export default meta;
type Story = StoryObj<typeof meta>;

const SAMPLE_TEXT =
  "着信: 0120-000-000\nSIP URI: sip:alice@203.0.113.10\nパスワード: hunter2";

function DemoEditable(props: { initialValue: string }) {
  const [value, setValue] = useState(props.initialValue);
  return (
    <MaskedTextEditor value={value} onChange={setValue} ariaLabel="入力テキスト" />
  );
}

export const Editable: Story = {
  args: {
    value: SAMPLE_TEXT,
    onChange: () => {},
    ariaLabel: "入力テキスト",
  },
  render: () => <DemoEditable initialValue={SAMPLE_TEXT} />,
};

export const ReadOnlyOutput: Story = {
  args: {
    value: SAMPLE_TEXT,
    readOnly: true,
    ariaLabel: "出力(マスク後)テキスト",
  },
};

export const Empty: Story = {
  args: {
    value: "",
    onChange: () => {},
    ariaLabel: "入力テキスト",
  },
  render: () => <DemoEditable initialValue="" />,
};
