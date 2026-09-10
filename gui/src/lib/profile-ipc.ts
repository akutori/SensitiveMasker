import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RuleListItem } from "@/components/rule-edit-screen";
import { fromRuleDto, toRuleDto, type RuleProfileDto } from "./masking-ipc";

export interface ProfileSummaryDto {
  id: number;
  name: string;
  rule_count: number;
  is_favorite: boolean;
  is_active: boolean;
  updated_at: string;
  tags: string[];
}

export interface ProfileDetail {
  name: string;
  description: string;
  rules: RuleListItem[];
}

function toProfileDto(name: string, description: string, rules: RuleListItem[]): RuleProfileDto {
  return { profile_name: name, description: description || null, rules: rules.map(toRuleDto) };
}

export async function isStoreInitialized(): Promise<boolean> {
  return invoke<boolean>("is_store_initialized");
}

// 2回目以降の起動用。ディスク上は初期化済みでも、プロセスを跨いで保持されない
// メモリ上のストア状態を実体化するために、起動のたびに一度呼ぶ必要がある。
export async function openStore(): Promise<void> {
  await invoke("open_store");
}

export async function initializeStore(): Promise<void> {
  await invoke("initialize_store");
}

export async function listProfiles(): Promise<ProfileSummaryDto[]> {
  return invoke<ProfileSummaryDto[]>("list_profiles");
}

export async function getProfile(name: string): Promise<ProfileDetail> {
  const dto = await invoke<RuleProfileDto>("get_profile", { name });
  return { name: dto.profile_name, description: dto.description ?? "", rules: dto.rules.map(fromRuleDto) };
}

export async function createProfile(
  name: string,
  description: string,
  rules: RuleListItem[]
): Promise<number> {
  return invoke<number>("create_profile", { profile: toProfileDto(name, description, rules) });
}

export async function updateProfile(
  oldName: string,
  name: string,
  description: string,
  rules: RuleListItem[]
): Promise<void> {
  await invoke("update_profile", { oldName, profile: toProfileDto(name, description, rules) });
}

export async function deleteProfile(name: string): Promise<void> {
  await invoke("delete_profile", { name });
}

export async function setActiveProfile(name: string): Promise<void> {
  await invoke("set_active_profile", { name });
}

export async function setFavorite(name: string, isFavorite: boolean): Promise<void> {
  await invoke("set_favorite", { name, isFavorite });
}

export async function listTags(): Promise<string[]> {
  return invoke<string[]>("list_tags");
}

export async function createTag(name: string): Promise<void> {
  await invoke("create_tag", { name });
}

export async function renameTag(oldName: string, newName: string): Promise<void> {
  await invoke("rename_tag", { oldName, newName });
}

export async function deleteTag(name: string): Promise<void> {
  await invoke("delete_tag", { name });
}

export async function setProfileTags(profileName: string, tags: string[]): Promise<void> {
  await invoke("set_profile_tags", { profileName, tags });
}

export async function exportProfileToFile(
  name: string,
  passphrase: string,
  destPath: string
): Promise<void> {
  await invoke("export_profile_to_file", { name, passphrase, destPath });
}

export async function exportAllToFile(passphrase: string, destPath: string): Promise<void> {
  await invoke("export_all_to_file", { passphrase, destPath });
}

export interface ImportEntryDto {
  original_name: string;
  resolved_name: string;
  renamed: boolean;
}

export type ImportPreviewDto =
  | { kind: "single"; name: string }
  | { kind: "all"; active_profile_name: string | null; entries: ImportEntryDto[] };

export async function previewImport(sourcePath: string, passphrase: string): Promise<ImportPreviewDto> {
  return invoke<ImportPreviewDto>("preview_import", { sourcePath, passphrase });
}

export async function commitPendingImport(): Promise<void> {
  await invoke("commit_pending_import");
}

// フォーカスの有無に関わらず、変更を検知したウィンドウ側で最新状態を取り直すためのイベント。
// 今は単一ウィンドウだが、将来の別ウィンドウ化でそのまま使う想定(PoCで検証済み)。
export function onProfilesChanged(handler: () => void): Promise<() => void> {
  return listen("profiles-changed", handler);
}

export function onTagsChanged(handler: () => void): Promise<() => void> {
  return listen("tags-changed", handler);
}
