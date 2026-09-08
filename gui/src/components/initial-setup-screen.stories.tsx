import type { Meta, StoryObj } from "@storybook/react-vite";
import { InitialSetupScreen } from "./initial-setup-screen";

const meta = {
  component: InitialSetupScreen,
  parameters: {
    layout: "fullscreen",
  },
} satisfies Meta<typeof InitialSetupScreen>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    onStart: () => console.log("start clicked"),
  },
};
