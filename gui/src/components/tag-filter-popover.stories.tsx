import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { TagFilterPopover } from "./tag-filter-popover";

const meta = {
  component: TagFilterPopover,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof TagFilterPopover>;

export default meta;
type Story = StoryObj<typeof meta>;

const AVAILABLE_TAGS = ["SIP", "案件A", "案件B", "検証用"];

function DemoTrigger(props: { initialSelected: string[] }) {
  const [selectedTags, setSelectedTags] = useState(props.initialSelected);
  return (
    <TagFilterPopover
      availableTags={AVAILABLE_TAGS}
      selectedTags={selectedTags}
      onSelectedTagsChange={setSelectedTags}
    />
  );
}

export const Default: Story = {
  args: {
    availableTags: AVAILABLE_TAGS,
    selectedTags: [],
    onSelectedTagsChange: () => {},
  },
  render: () => <DemoTrigger initialSelected={[]} />,
};

export const WithSelection: Story = {
  args: {
    availableTags: AVAILABLE_TAGS,
    selectedTags: ["SIP", "案件B"],
    onSelectedTagsChange: () => {},
  },
  render: () => <DemoTrigger initialSelected={["SIP", "案件B"]} />,
};
