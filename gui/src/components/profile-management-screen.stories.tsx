import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  ProfileManagementScreen,
  type Profile,
  type SortOption,
} from "./profile-management-screen";

const meta = {
  component: ProfileManagementScreen,
  parameters: {
    layout: "fullscreen",
  },
} satisfies Meta<typeof ProfileManagementScreen>;

export default meta;
type Story = StoryObj<typeof meta>;

const SORT_OPTIONS: SortOption[] = [
  { value: "updated_desc", label: "更新日時が新しい順" },
  { value: "updated_asc", label: "更新日時が古い順" },
  { value: "name_asc", label: "名前順" },
];

const AVAILABLE_TAGS = ["SIP", "案件A", "案件B", "検証用"];

const INITIAL_PROFILES: Profile[] = [
  {
    id: "1",
    name: "SIP監視用",
    isActive: true,
    isFavorite: true,
    updatedAt: "2026-09-01",
    ruleCount: 5,
    tags: ["SIP", "案件A"],
  },
  {
    id: "2",
    name: "Asterisk本番ログ",
    isActive: false,
    isFavorite: false,
    updatedAt: "2026-08-20",
    ruleCount: 3,
    tags: ["SIP", "案件B"],
  },
  {
    id: "3",
    name: "検証用サンプル",
    isActive: false,
    isFavorite: false,
    updatedAt: "2026-07-10",
    ruleCount: 2,
    tags: ["案件A"],
  },
];

function DemoScreen(props: { initialProfiles: Profile[] }) {
  const [profiles, setProfiles] = useState(props.initialProfiles);
  const [searchQuery, setSearchQuery] = useState("");
  const [sortValue, setSortValue] = useState("updated_desc");
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [selectedTags, setSelectedTags] = useState<string[]>([]);

  return (
    <ProfileManagementScreen
      profiles={profiles}
      searchQuery={searchQuery}
      onSearchQueryChange={setSearchQuery}
      sortOptions={SORT_OPTIONS}
      sortValue={sortValue}
      onSortValueChange={setSortValue}
      favoritesOnly={favoritesOnly}
      onFavoritesOnlyChange={setFavoritesOnly}
      availableTags={AVAILABLE_TAGS}
      selectedTags={selectedTags}
      onSelectedTagsChange={setSelectedTags}
      onClose={() => console.log("close")}
      onNewProfile={() => console.log("new profile")}
      onCreateFromTemplate={() => console.log("create from template")}
      onManageTags={() => console.log("manage tags")}
      onExportAll={() => console.log("export all")}
      onImport={() => console.log("import")}
      onToggleFavorite={(id) =>
        setProfiles(
          profiles.map((p) => (p.id === id ? { ...p, isFavorite: !p.isFavorite } : p))
        )
      }
      onRowClick={(id) =>
        setProfiles(profiles.map((p) => ({ ...p, isActive: p.id === id })))
      }
      onEditProfile={(id) => console.log("edit", id)}
      onDuplicateProfile={(id, newName) => {
        const source = profiles.find((p) => p.id === id);
        if (!source) return;
        setProfiles([
          ...profiles,
          {
            ...source,
            id: crypto.randomUUID(),
            name: newName,
            isActive: false,
            isFavorite: false,
          },
        ]);
      }}
      onExportProfile={(id) => console.log("export", id)}
      onDeleteProfile={(id) => setProfiles(profiles.filter((p) => p.id !== id))}
      onProfileTagsChange={(id, tags) =>
        setProfiles(profiles.map((p) => (p.id === id ? { ...p, tags } : p)))
      }
    />
  );
}

export const Default: Story = {
  args: {
    profiles: INITIAL_PROFILES,
    searchQuery: "",
    onSearchQueryChange: () => {},
    sortOptions: SORT_OPTIONS,
    sortValue: "updated_desc",
    onSortValueChange: () => {},
    favoritesOnly: false,
    onFavoritesOnlyChange: () => {},
    availableTags: AVAILABLE_TAGS,
    selectedTags: [],
    onSelectedTagsChange: () => {},
    onClose: () => {},
    onNewProfile: () => {},
    onCreateFromTemplate: () => {},
    onManageTags: () => {},
    onExportAll: () => {},
    onImport: () => {},
    onToggleFavorite: () => {},
    onRowClick: () => {},
    onEditProfile: () => {},
    onDuplicateProfile: () => {},
    onExportProfile: () => {},
    onDeleteProfile: () => {},
    onProfileTagsChange: () => {},
  },
  render: () => <DemoScreen initialProfiles={INITIAL_PROFILES} />,
};

export const EmptyState: Story = {
  args: {
    ...Default.args,
    profiles: [],
  },
};
