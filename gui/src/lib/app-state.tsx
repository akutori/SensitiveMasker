import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import type { RuleListItem } from "@/components/rule-edit-screen";
import { DEMO_SAMPLE_TEXT, PROFILE_TEMPLATE_RULES } from "./demo-seed-data";
import { simulateMask } from "./demo-masking";

export interface Profile {
  id: string;
  name: string;
  description: string;
  isActive: boolean;
  isFavorite: boolean;
  updatedAt: string;
  tags: string[];
}

export interface Tag {
  id: string;
  name: string;
}

export function resolveUniqueName(baseName: string, existingNames: string[]): string {
  if (!existingNames.includes(baseName)) return baseName;
  let candidate = `${baseName} (インポート)`;
  let suffix = 2;
  while (existingNames.includes(candidate)) {
    candidate = `${baseName} (インポート ${suffix})`;
    suffix += 1;
  }
  return candidate;
}

function today(): string {
  return new Date().toISOString().slice(0, 10);
}

function withRuleIds(rules: Omit<RuleListItem, "id">[]): RuleListItem[] {
  return rules.map((rule) => ({ ...rule, id: crypto.randomUUID() }));
}

export interface AppStateValue {
  initialized: boolean;
  start: () => Promise<void>;

  profiles: Profile[];
  activeProfileId: string | null;
  setActiveProfileId: (id: string) => Promise<void>;
  createProfile: (name: string, templateValue?: string) => Promise<string>;
  duplicateProfile: (id: string, newName: string) => Promise<string>;
  deleteProfile: (id: string) => Promise<void>;
  toggleFavorite: (id: string) => Promise<void>;
  updateProfileMeta: (
    id: string,
    meta: { name: string; description: string }
  ) => Promise<void>;

  tags: Tag[];
  createTag: (name: string) => Promise<void>;
  renameTag: (id: string, newName: string) => Promise<void>;
  deleteTag: (id: string) => Promise<void>;

  rulesByProfileId: Record<string, RuleListItem[]>;
  saveRules: (profileId: string, rules: RuleListItem[]) => Promise<void>;

  inputText: string;
  setInputText: (text: string) => void;
  outputText: string;
  statusText: string;
  runMask: () => void;
  clearInput: () => void;
}

const AppStateContext = createContext<AppStateValue | null>(null);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [initialized, setInitialized] = useState(false);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [activeProfileId, setActiveProfileIdState] = useState<string | null>(null);
  const [tags, setTags] = useState<Tag[]>([]);
  const [rulesByProfileId, setRulesByProfileId] = useState<Record<string, RuleListItem[]>>({});
  const [inputText, setInputText] = useState(DEMO_SAMPLE_TEXT);
  const [outputText, setOutputText] = useState("");
  const [statusText, setStatusText] = useState("アクティブプロファイル: なし");

  const value = useMemo<AppStateValue>(
    () => ({
      initialized,
      start: async () => {
        setInitialized(true);
      },

      profiles,
      activeProfileId,
      setActiveProfileId: async (id) => {
        setActiveProfileIdState(id);
        setProfiles((prev) => prev.map((p) => ({ ...p, isActive: p.id === id })));
      },
      createProfile: async (name, templateValue) => {
        const id = crypto.randomUUID();
        setProfiles((prev) => [
          ...prev.map((p) => ({ ...p, isActive: false })),
          {
            id,
            name,
            description: "",
            isActive: true,
            isFavorite: false,
            updatedAt: today(),
            tags: [],
          },
        ]);
        setActiveProfileIdState(id);
        const seedRules = templateValue ? PROFILE_TEMPLATE_RULES[templateValue] : undefined;
        setRulesByProfileId((prev) => ({
          ...prev,
          [id]: seedRules ? withRuleIds(seedRules) : [],
        }));
        return id;
      },
      duplicateProfile: async (id, newName) => {
        const source = profiles.find((p) => p.id === id);
        const newId = crypto.randomUUID();
        setProfiles((prev) => [
          ...prev,
          {
            id: newId,
            name: newName,
            description: source?.description ?? "",
            isActive: false,
            isFavorite: false,
            updatedAt: today(),
            tags: source?.tags ?? [],
          },
        ]);
        setRulesByProfileId((prev) => ({
          ...prev,
          [newId]: (rulesByProfileId[id] ?? []).map((rule) => ({
            ...rule,
            id: crypto.randomUUID(),
          })),
        }));
        return newId;
      },
      deleteProfile: async (id) => {
        setProfiles((prev) => prev.filter((p) => p.id !== id));
        setRulesByProfileId((prev) => {
          const next = { ...prev };
          delete next[id];
          return next;
        });
        setActiveProfileIdState((prev) => (prev === id ? null : prev));
      },
      toggleFavorite: async (id) => {
        setProfiles((prev) =>
          prev.map((p) => (p.id === id ? { ...p, isFavorite: !p.isFavorite } : p))
        );
      },
      updateProfileMeta: async (id, meta) => {
        setProfiles((prev) =>
          prev.map((p) =>
            p.id === id
              ? { ...p, name: meta.name, description: meta.description, updatedAt: today() }
              : p
          )
        );
      },

      tags,
      createTag: async (name) => {
        setTags((prev) => [...prev, { id: crypto.randomUUID(), name }]);
      },
      renameTag: async (id, newName) => {
        const oldName = tags.find((t) => t.id === id)?.name;
        setTags((prev) => prev.map((t) => (t.id === id ? { ...t, name: newName } : t)));
        if (oldName) {
          setProfiles((prev) =>
            prev.map((p) => ({
              ...p,
              tags: p.tags.map((t) => (t === oldName ? newName : t)),
            }))
          );
        }
      },
      deleteTag: async (id) => {
        const name = tags.find((t) => t.id === id)?.name;
        setTags((prev) => prev.filter((t) => t.id !== id));
        if (name) {
          setProfiles((prev) =>
            prev.map((p) => ({ ...p, tags: p.tags.filter((t) => t !== name) }))
          );
        }
      },

      rulesByProfileId,
      saveRules: async (profileId, rules) => {
        setRulesByProfileId((prev) => ({ ...prev, [profileId]: rules }));
        setProfiles((prev) =>
          prev.map((p) => (p.id === profileId ? { ...p, updatedAt: today() } : p))
        );
      },

      inputText,
      setInputText,
      outputText,
      statusText,
      runMask: () => {
        const activeProfile = profiles.find((p) => p.id === activeProfileId);
        const rules = activeProfileId ? (rulesByProfileId[activeProfileId] ?? []) : [];
        const { text, matchCounts } = simulateMask(inputText, rules);
        const totalMatches = Object.values(matchCounts).reduce((sum, n) => sum + n, 0);
        setOutputText(text);
        setStatusText(
          `アクティブプロファイル: ${activeProfile?.name ?? "なし"} ・ 直近のマスク実行でマッピング${totalMatches}件を置換`
        );
      },
      clearInput: () => setInputText(""),
    }),
    [initialized, profiles, activeProfileId, tags, rulesByProfileId, inputText, outputText, statusText]
  );

  return <AppStateContext.Provider value={value}>{children}</AppStateContext.Provider>;
}

export function useAppState(): AppStateValue {
  const ctx = useContext(AppStateContext);
  if (!ctx) throw new Error("useAppState must be used within AppStateProvider");
  return ctx;
}
