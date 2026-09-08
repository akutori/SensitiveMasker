import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "@/components/ui/button";
import { TagManagementDialog, type Tag } from "./tag-management-dialog";

const meta = {
  component: TagManagementDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof TagManagementDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const INITIAL_TAGS: Tag[] = [
  { id: "1", name: "SIP" },
  { id: "2", name: "案件A" },
  { id: "3", name: "案件B" },
];

function DemoTrigger(props: { initialTags: Tag[]; label: string }) {
  const [open, setOpen] = useState(false);
  const [tags, setTags] = useState(props.initialTags);
  const [searchQuery, setSearchQuery] = useState("");
  const [newTagName, setNewTagName] = useState("");
  const [errorMessage, setErrorMessage] = useState<string | undefined>();
  const [invalidTagId, setInvalidTagId] = useState<string | "new" | undefined>();

  const isDuplicate = (name: string, excludeId?: string) =>
    tags.some((tag) => tag.name === name && tag.id !== excludeId);

  return (
    <>
      <Button
        variant="outline"
        onClick={() => {
          setTags(props.initialTags);
          setSearchQuery("");
          setNewTagName("");
          setErrorMessage(undefined);
          setInvalidTagId(undefined);
          setOpen(true);
        }}
      >
        {props.label}
      </Button>
      <TagManagementDialog
        open={open}
        onOpenChange={setOpen}
        tags={tags}
        searchQuery={searchQuery}
        onSearchQueryChange={setSearchQuery}
        newTagName={newTagName}
        onNewTagNameChange={(name) => {
          setNewTagName(name);
          setErrorMessage(undefined);
          setInvalidTagId(undefined);
        }}
        onAddTag={() => {
          if (isDuplicate(newTagName)) {
            setErrorMessage("同じ名前のタグが既に存在します");
            setInvalidTagId("new");
            return;
          }
          setTags([...tags, { id: crypto.randomUUID(), name: newTagName }]);
          setNewTagName("");
          setErrorMessage(undefined);
          setInvalidTagId(undefined);
        }}
        onRenameTag={(id, newName) => {
          if (isDuplicate(newName, id)) {
            setErrorMessage("同じ名前のタグが既に存在します");
            setInvalidTagId(id);
            return;
          }
          setTags(tags.map((tag) => (tag.id === id ? { ...tag, name: newName } : tag)));
          setErrorMessage(undefined);
          setInvalidTagId(undefined);
        }}
        onDeleteTag={(id) => setTags(tags.filter((tag) => tag.id !== id))}
        errorMessage={errorMessage}
        invalidTagId={invalidTagId}
      />
    </>
  );
}

export const Default: Story = {
  args: {
    open: false,
    onOpenChange: () => {},
    tags: INITIAL_TAGS,
    searchQuery: "",
    onSearchQueryChange: () => {},
    newTagName: "",
    onNewTagNameChange: () => {},
    onAddTag: () => {},
    onRenameTag: () => {},
    onDeleteTag: () => {},
  },
  render: () => <DemoTrigger initialTags={INITIAL_TAGS} label="タグ管理" />,
};

export const OpenByDefault: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    tags: INITIAL_TAGS,
    searchQuery: "",
    onSearchQueryChange: () => {},
    newTagName: "",
    onNewTagNameChange: () => {},
    onAddTag: () => {},
    onRenameTag: () => {},
    onDeleteTag: () => {},
  },
};

export const EmptyState: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    tags: [],
    searchQuery: "",
    onSearchQueryChange: () => {},
    newTagName: "",
    onNewTagNameChange: () => {},
    onAddTag: () => {},
    onRenameTag: () => {},
    onDeleteTag: () => {},
  },
};

const MANY_TAGS: Tag[] = Array.from({ length: 30 }, (_, i) => ({
  id: String(i + 1),
  name: `タグ${i + 1}`,
}));

export const ManyTags: Story = {
  args: {
    open: true,
    onOpenChange: () => {},
    tags: MANY_TAGS,
    searchQuery: "",
    onSearchQueryChange: () => {},
    newTagName: "",
    onNewTagNameChange: () => {},
    onAddTag: () => {},
    onRenameTag: () => {},
    onDeleteTag: () => {},
  },
};
