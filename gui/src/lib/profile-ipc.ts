import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RuleListItem } from "@/components/rule-edit-screen";
import { fromRuleDto, toRuleDto, type RuleProfileDto } from "./masking-ipc";

export interface ProfileSummaryDto {
  id: number;
  name: string;
  rule_count: number;
  enabled_rule_count: number;
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

// インポート確認画面でルールの中身を表示するための型。
// 「構文的に有効だが実データの書式と食い違う」細工されたルールに、確定前に
// 気付けるようにするための情報であり、確定前に必ず提示する。
export interface ImportRuleDto {
  name: string;
  pattern_type: "literal" | "regex";
  pattern: string;
  mode: "fixed" | "sequential";
  fixed_value: string | null;
  prefix: string | null;
  enabled: boolean;
}

// Rust側のExportImportErrorに対応する型。invalid_inputはパスフレーズによる復号を
// 試みる前の事前検証(パス形式・拡張子・サイズ・保存先)で弾かれたことを示すため、
// 呼び出し元はこれをパスフレーズ入力エラーとして表示してはならない。
export interface ExportImportError {
  kind: "invalid_input" | "failed";
  message: string;
}

export function isExportImportError(error: unknown): error is ExportImportError {
  if (typeof error !== "object" || error === null) return false;
  const { kind, message } = error as Record<string, unknown>;
  return (kind === "invalid_input" || kind === "failed") && typeof message === "string";
}

export interface ImportEntryDto {
  original_name: string;
  resolved_name: string;
  renamed: boolean;
  rules: ImportRuleDto[];
  tags: string[];
}

export type ImportPreviewDto =
  | { kind: "single"; name: string; rules: ImportRuleDto[]; tags: string[] }
  | { kind: "all"; will_activate_profile_name: string | null; entries: ImportEntryDto[] };

export async function previewImport(sourcePath: string, passphrase: string): Promise<ImportPreviewDto> {
  return invoke<ImportPreviewDto>("preview_import", { sourcePath, passphrase });
}

export interface CommitImportResultDto {
  activated_profile_name: string | null;
}

export async function commitPendingImport(): Promise<CommitImportResultDto> {
  return invoke<CommitImportResultDto>("commit_pending_import");
}

// 確認ダイアログのキャンセル・離脱時に呼ぶ。preview_importが復号した平文を
// プロセス内に残さないためと、キャンセル後にcommitPendingImportを呼んでも
// 確定しないようにするため。
export async function clearPendingImport(): Promise<void> {
  await invoke("clear_pending_import");
}

// フォーカスの有無に関わらず、変更を検知したウィンドウ側で最新状態を取り直すためのイベント。
// 今は単一ウィンドウだが、将来の別ウィンドウ化でそのまま使う想定(PoCで検証済み)。
export function onProfilesChanged(handler: () => void): Promise<() => void> {
  return listen("profiles-changed", handler);
}

export function onTagsChanged(handler: () => void): Promise<() => void> {
  return listen("tags-changed", handler);
}

export interface ActiveProfileRulesWeakenedPayload {
  profileName: string;
  weakenedRuleNames: string[];
}

// update_profileの呼び出し元(通常の編集操作、または直接IPCを叩く経路の両方を問わず)が
// アクティブプロファイルの既存の有効ルールを無効化・削除、またはマスク挙動を左右する
// 内容(pattern等)を書き換えた場合にRust側から届く。マスクルールが無言で無力化される
// のに気付く簡易な手がかりとして使う(SensitiveMasker側の設計、改ざん耐性のある記録
// ではなく即時の通知であることが重要)。
export function onActiveProfileRulesWeakened(
  handler: (payload: ActiveProfileRulesWeakenedPayload) => void
): Promise<() => void> {
  return listen<ActiveProfileRulesWeakenedPayload>("active-profile-rules-weakened", (event) =>
    handler(event.payload)
  );
}
