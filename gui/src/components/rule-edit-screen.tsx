import { useId, useState, type CSSProperties } from "react";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Checkbox } from "@/components/ui/checkbox";
import {
  RuleEditDialog,
  type PatternType,
  type RuleFormValues,
  type RuleMode,
  type RuleTemplateOption,
} from "./rule-edit-dialog";
import { DeleteConfirmDialog } from "./delete-confirm-dialog";
import { ConfirmDialog } from "./confirm-dialog";
import { TagFilterPopover } from "./tag-filter-popover";

export type RuleListItem = RuleFormValues & { id: string };

const EMPTY_RULE_VALUES: RuleFormValues = {
  name: "",
  patternType: "literal",
  pattern: "",
  mode: "fixed",
  fixedValue: "",
  prefix: "",
  enabled: true,
  description: "",
};

export function patternTypeLabel(patternType: PatternType) {
  return patternType === "regex" ? "正規表現" : "リテラル";
}

export function modeLabel(mode: RuleMode) {
  return mode === "sequential" ? "連番" : "固定";
}

export interface RuleEditScreenProps {
  profileName: string;
  onProfileNameChange: (name: string) => void;
  profileDescription: string;
  onProfileDescriptionChange: (description: string) => void;
  availableTags: string[];
  profileTags: string[];
  onProfileTagsChange: (tags: string[]) => void;
  rules: RuleListItem[];
  onReorderRules: (rules: RuleListItem[]) => void;
  onToggleRuleEnabled: (id: string) => void;
  onAddRule: (values: RuleFormValues) => void;
  onEditRule: (id: string, values: RuleFormValues) => void;
  onDeleteRule: (id: string) => void;
  ruleTemplateOptions: RuleTemplateOption[];
  onResolveRuleTemplate: (templateValue: string) => Partial<RuleFormValues>;
  sampleText: string;
  onSampleTextChange: (text: string) => void;
  maskedResult: string;
  onSave: () => void;
  onCancel: () => void;
}

function SortableRuleRow({
  rule,
  onToggleEnabled,
  onEdit,
  onDuplicate,
  onDelete,
}: {
  rule: RuleListItem;
  onToggleEnabled: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } =
    useSortable({ id: rule.id });

  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  };

  return (
    <div ref={setNodeRef} style={style} className="rounded-lg border border-input p-2">
      <div className="flex items-start gap-2">
        <button
          {...attributes}
          {...listeners}
          aria-label="ドラッグして並べ替え"
          className="mt-1 cursor-grab touch-none text-foreground"
        >
          <GripVertical className="size-4" />
        </button>
        <Checkbox
          checked={rule.enabled}
          onCheckedChange={onToggleEnabled}
          aria-label={rule.enabled ? "ルールを無効化" : "ルールを有効化"}
          className="mt-1"
        />
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium">
            <bdi>{rule.name}</bdi>
          </div>
          <div className="mt-0.5 text-xs text-foreground">
            {patternTypeLabel(rule.patternType)} / {modeLabel(rule.mode)} ・ 説明:{" "}
            {/* bdi: 双方向書式文字を含む名前・説明(インポート由来を含む)が、直後の
                「(無効中)」表示の位置を偽装できないようにする。 */}
            <bdi>{rule.description}</bdi>
            {!rule.enabled && "(無効中)"}
          </div>
        </div>
        <div className="flex shrink-0 gap-1">
          <Button size="sm" variant="outline" onClick={onEdit}>
            編集
          </Button>
          <Button size="sm" variant="outline" onClick={onDuplicate}>
            複製
          </Button>
          <Button size="sm" variant="outline" onClick={onDelete}>
            削除
          </Button>
        </div>
      </div>
    </div>
  );
}

export function RuleEditScreen({
  profileName,
  onProfileNameChange,
  profileDescription,
  onProfileDescriptionChange,
  availableTags,
  profileTags,
  onProfileTagsChange,
  rules,
  onReorderRules,
  onToggleRuleEnabled,
  onAddRule,
  onEditRule,
  onDeleteRule,
  ruleTemplateOptions,
  onResolveRuleTemplate,
  sampleText,
  onSampleTextChange,
  maskedResult,
  onSave,
  onCancel,
}: RuleEditScreenProps) {
  const id = useId();
  const [initialSnapshot] = useState(() =>
    JSON.stringify({ profileName, profileDescription, profileTags, rules })
  );
  const hasChanges =
    JSON.stringify({ profileName, profileDescription, profileTags, rules }) !== initialSnapshot;

  const [showDiscardConfirm, setShowDiscardConfirm] = useState(false);
  const [pendingDeleteRule, setPendingDeleteRule] = useState<RuleListItem | null>(null);
  const [editingRuleId, setEditingRuleId] = useState<string | "new" | null>(null);
  const [ruleFormValues, setRuleFormValues] = useState<RuleFormValues>(EMPTY_RULE_VALUES);
  const [ruleFormError, setRuleFormError] = useState<string | undefined>();

  const sensors = useSensors(
    useSensor(PointerSensor),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    if (over && active.id !== over.id) {
      const oldIndex = rules.findIndex((r) => r.id === active.id);
      const newIndex = rules.findIndex((r) => r.id === over.id);
      onReorderRules(arrayMove(rules, oldIndex, newIndex));
    }
  };

  const startAddRule = () => {
    setEditingRuleId("new");
    setRuleFormValues(EMPTY_RULE_VALUES);
    setRuleFormError(undefined);
  };

  const startEditRule = (rule: RuleListItem) => {
    setEditingRuleId(rule.id);
    setRuleFormValues(rule);
    setRuleFormError(undefined);
  };

  const startDuplicateRule = (rule: RuleListItem) => {
    setEditingRuleId("new");
    setRuleFormValues({ ...rule, name: `${rule.name} のコピー` });
    setRuleFormError(undefined);
  };

  const confirmRuleEdit = () => {
    const isDuplicateName = rules.some(
      (r) => r.name === ruleFormValues.name && r.id !== editingRuleId
    );
    if (isDuplicateName) {
      setRuleFormError("入力エラー: 同じ名前のルールが既に存在します");
      return;
    }
    if (editingRuleId === "new") {
      onAddRule(ruleFormValues);
    } else if (editingRuleId) {
      onEditRule(editingRuleId, ruleFormValues);
    }
    setEditingRuleId(null);
  };

  const handleCancel = () => {
    if (hasChanges) {
      setShowDiscardConfirm(true);
    } else {
      onCancel();
    }
  };

  return (
    <div className="p-5">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-bold">ルール編集</h1>
        <div className="flex gap-2">
          <Button onClick={onSave}>保存</Button>
          <Button variant="outline" onClick={handleCancel}>
            キャンセル
          </Button>
        </div>
      </div>

      <div className="mt-5 flex flex-wrap gap-4">
        <div className="grid gap-1.5">
          <Label htmlFor={`${id}-profile-name`}>プロファイル名:</Label>
          <Input
            id={`${id}-profile-name`}
            value={profileName}
            onChange={(e) => onProfileNameChange(e.target.value)}
            className="w-72"
          />
        </div>
        <div className="grid flex-1 gap-1.5">
          <Label htmlFor={`${id}-profile-description`}>説明:</Label>
          <Input
            id={`${id}-profile-description`}
            value={profileDescription}
            onChange={(e) => onProfileDescriptionChange(e.target.value)}
          />
        </div>
        <div className="grid gap-1.5">
          <Label>タグ:</Label>
          <TagFilterPopover
            availableTags={availableTags}
            selectedTags={profileTags}
            onSelectedTagsChange={onProfileTagsChange}
          />
        </div>
      </div>

      <div className="mt-5 flex flex-wrap gap-6 border-t pt-5">
        <div className="min-w-80 flex-1">
          <h2 className="text-sm font-bold">ルール一覧(ドラッグで並べ替え)</h2>

          <div className="mt-3 grid gap-2">
            {rules.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                ルールがまだありません。「+ ルールを追加」から作成してください
              </p>
            ) : (
              <DndContext
                sensors={sensors}
                collisionDetection={closestCenter}
                onDragEnd={handleDragEnd}
              >
                <SortableContext
                  items={rules.map((r) => r.id)}
                  strategy={verticalListSortingStrategy}
                >
                  {rules.map((rule) => (
                    <SortableRuleRow
                      key={rule.id}
                      rule={rule}
                      onToggleEnabled={() => onToggleRuleEnabled(rule.id)}
                      onEdit={() => startEditRule(rule)}
                      onDuplicate={() => startDuplicateRule(rule)}
                      onDelete={() => setPendingDeleteRule(rule)}
                    />
                  ))}
                </SortableContext>
              </DndContext>
            )}
          </div>

          <Button variant="outline" className="mt-3" onClick={startAddRule}>
            + ルールを追加
          </Button>
        </div>

        <div className="min-w-80 flex-1">
          <h2 className="text-sm font-bold">
            プレビュー(全ルール適用結果をリアルタイム表示)
          </h2>

          <div className="mt-3 grid gap-1.5">
            <Label htmlFor={`${id}-sample-text`}>サンプルテキスト:</Label>
            <Textarea
              id={`${id}-sample-text`}
              value={sampleText}
              onChange={(e) => onSampleTextChange(e.target.value)}
              className="h-40 bg-muted/50"
            />
          </div>

          <div className="mt-3 grid gap-1.5">
            <div className="flex items-center justify-between">
              <Label htmlFor={`${id}-masked-result`}>マスク結果:</Label>
              {rules.every((r) => !r.enabled) && rules.length > 0 && (
                <span className="text-xs text-muted-foreground">
                  有効なルールがないため、変換されません
                </span>
              )}
            </div>
            <Textarea
              id={`${id}-masked-result`}
              value={maskedResult}
              readOnly
              className="h-40 bg-muted"
            />
          </div>
        </div>
      </div>

      <DeleteConfirmDialog
        open={pendingDeleteRule !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setPendingDeleteRule(null);
        }}
        targetType="ルール"
        targetName={pendingDeleteRule?.name ?? ""}
        onConfirm={() => {
          if (pendingDeleteRule) onDeleteRule(pendingDeleteRule.id);
          setPendingDeleteRule(null);
        }}
      />

      <RuleEditDialog
        open={editingRuleId !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setEditingRuleId(null);
        }}
        values={ruleFormValues}
        onValuesChange={(values) => {
          setRuleFormValues(values);
          setRuleFormError(undefined);
        }}
        templateOptions={ruleTemplateOptions}
        onTemplateSelect={(templateValue) => {
          setRuleFormValues((prev) => ({
            ...prev,
            ...onResolveRuleTemplate(templateValue),
          }));
        }}
        errorMessage={ruleFormError}
        invalidField={ruleFormError ? "name" : undefined}
        onConfirm={confirmRuleEdit}
      />

      <ConfirmDialog
        open={showDiscardConfirm}
        onOpenChange={setShowDiscardConfirm}
        title="変更の破棄確認"
        description="編集中の内容が保存されていません。破棄してもよろしいですか?"
        onConfirm={() => {
          setShowDiscardConfirm(false);
          onCancel();
        }}
      />
    </div>
  );
}
