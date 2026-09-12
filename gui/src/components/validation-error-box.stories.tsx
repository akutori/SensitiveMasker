import type { Meta, StoryObj } from "@storybook/react-vite";
import { ValidationErrorBox } from "./validation-error-box";

const meta = {
  component: ValidationErrorBox,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ValidationErrorBox>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ShortMessage: Story = {
  args: {
    message: "パスフレーズが一致しません。",
  },
};

export const LongMessage: Story = {
  args: {
    message:
      "ファイルの内容が不正です(サイズが9バイト、期待値は32バイトです)。正しいエクスポートファイルを選択し直してください。",
  },
};
