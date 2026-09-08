import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { cn } from "cn";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { Button } from "@/components/ui/button";
import { ValidationErrorBox } from "./validation-error-box";
import { ConfirmDialog } from "./confirm-dialog";

export type PatternType = "literal" | "regex";
export type RuleMode = "fixed" | "sequential";

export interface RuleFormValues {
  name: string;
  patternType: PatternType;
  pattern: string;
  mode: RuleMode;
  fixedValue: string;
  prefix: string;
  enabled: boolean;
  description: string;
}

export interface RuleTemplateOption {
  value: string;
  label: string;
}

export interface RuleEditDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  values: RuleFormValues;
  onValuesChange: (values: RuleFormValues) => void;
  templateOptions: RuleTemplateOption[];
  onTemplateSelect: (templateValue: string) => void;
  errorMessage?: string;
  invalidField?: keyof RuleFormValues;
  onConfirm: () => void;
}

function FieldLabel({
  htmlFor,
  tooltip,
  muted,
  children,
}: {
  htmlFor: string;
  tooltip: string;
  muted?: boolean;
  children: ReactNode;
}) {
  const tooltipId = `${htmlFor}-tip`;
  return (
    <>
      <Tooltip>
        <TooltipTrigger asChild>
          <Label
            htmlFor={htmlFor}
            className={cn("w-fit cursor-help", muted && "text-muted-foreground")}
          >
            {children}
          </Label>
        </TooltipTrigger>
        <TooltipContent>{tooltip}</TooltipContent>
      </Tooltip>
      {/* Radixのツールチップ本体は非表示時にDOMから外れるため、
          aria-describedbyの参照先としてsr-onlyの説明を常時レンダリングする */}
      <span id={tooltipId} className="sr-only">
        {tooltip}
      </span>
    </>
  );
}

function describedBy(id: string) {
  return `${id}-tip`;
}

const NON_ENUM_FIELDS: (keyof RuleFormValues)[] = [
  "name",
  "pattern",
  "fixedValue",
  "prefix",
  "description",
];

function hasMeaningfulInput(values: RuleFormValues) {
  return NON_ENUM_FIELDS.some((key) => values[key].toString().trim().length > 0);
}

type PendingAction = { kind: "close" } | { kind: "template"; value: string };

export function RuleEditDialog({
  open,
  onOpenChange,
  values,
  onValuesChange,
  templateOptions,
  onTemplateSelect,
  errorMessage,
  invalidField,
  onConfirm,
}: RuleEditDialogProps) {
  const id = useId();
  const isFixed = values.mode === "fixed";
  const [pendingAction, setPendingAction] = useState<PendingAction | null>(null);
  const initialValuesRef = useRef(values);

  useEffect(() => {
    if (open) {
      initialValuesRef.current = values;
    }
    // 開いた瞬間の値だけをベースラインとして記録する(入力中の再計算はしない)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const set = <K extends keyof RuleFormValues>(key: K, value: RuleFormValues[K]) =>
    onValuesChange({ ...values, [key]: value });

  const isRequiredFieldMissing =
    values.name.trim().length === 0 ||
    values.pattern.trim().length === 0 ||
    (isFixed ? values.fixedValue.trim().length === 0 : values.prefix.trim().length === 0);

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen && JSON.stringify(values) !== JSON.stringify(initialValuesRef.current)) {
      setPendingAction({ kind: "close" });
      return;
    }
    onOpenChange(nextOpen);
  };

  const handleTemplateSelect = (value: string) => {
    if (hasMeaningfulInput(values)) {
      setPendingAction({ kind: "template", value });
    } else {
      onTemplateSelect(value);
    }
  };

  const confirmPendingAction = () => {
    if (pendingAction?.kind === "close") {
      onOpenChange(false);
    } else if (pendingAction?.kind === "template") {
      onTemplateSelect(pendingAction.value);
    }
    setPendingAction(null);
  };

  return (
    <TooltipProvider>
      <Dialog open={open} onOpenChange={handleOpenChange}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>ルールを追加/編集</DialogTitle>
          </DialogHeader>

          <div className="grid gap-3">
            <div className="grid grid-cols-[160px_1fr] items-center gap-x-3 gap-y-1">
              <FieldLabel
                htmlFor={`${id}-template`}
                tooltip="選択すると、名前・種別・パターン等の項目に組み込みの設定値が自動入力されます"
              >
                テンプレートから入力:
              </FieldLabel>
              <Select value="" onValueChange={handleTemplateSelect}>
                <SelectTrigger
                  id={`${id}-template`}
                  className="w-full"
                  aria-describedby={describedBy(`${id}-template`)}
                >
                  <SelectValue placeholder="(なし)" />
                </SelectTrigger>
                <SelectContent>
                  {templateOptions.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>

              <FieldLabel
                htmlFor={`${id}-name`}
                tooltip="このルールを識別するための名前です(プロファイル内で重複できません)"
              >
                名前:
              </FieldLabel>
              <Input
                id={`${id}-name`}
                value={values.name}
                aria-describedby={describedBy(`${id}-name`)}
                aria-invalid={invalidField === "name"}
                onChange={(e) => set("name", e.target.value)}
              />

              <FieldLabel
                htmlFor={`${id}-pattern-type`}
                tooltip="パターンを「リテラル」(完全一致)または「正規表現」として扱うかを選びます"
              >
                種別:
              </FieldLabel>
              <Select
                value={values.patternType}
                onValueChange={(v) => set("patternType", v as PatternType)}
              >
                <SelectTrigger
                  id={`${id}-pattern-type`}
                  className="w-full"
                  aria-describedby={describedBy(`${id}-pattern-type`)}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="literal">リテラル</SelectItem>
                  <SelectItem value="regex">正規表現</SelectItem>
                </SelectContent>
              </Select>

              <FieldLabel
                htmlFor={`${id}-pattern`}
                tooltip={
                  values.patternType === "regex"
                    ? "例: マスクしたい値の正規表現パターンを入力します"
                    : "マスクしたい値と完全に一致する文字列をそのまま入力します(正規表現の特殊文字もそのまま扱われます)"
                }
              >
                パターン:
              </FieldLabel>
              <Input
                id={`${id}-pattern`}
                value={values.pattern}
                aria-describedby={describedBy(`${id}-pattern`)}
                aria-invalid={invalidField === "pattern"}
                onChange={(e) => set("pattern", e.target.value)}
              />

              <FieldLabel
                htmlFor={`${id}-mode`}
                tooltip="「固定」は常に同じ値に、「連番」は検出順に連番付きの値に置き換えます"
              >
                モード:
              </FieldLabel>
              <Select value={values.mode} onValueChange={(v) => set("mode", v as RuleMode)}>
                <SelectTrigger
                  id={`${id}-mode`}
                  className="w-full"
                  aria-describedby={describedBy(`${id}-mode`)}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="fixed">固定</SelectItem>
                  <SelectItem value="sequential">連番</SelectItem>
                </SelectContent>
              </Select>

              <FieldLabel
                htmlFor={`${id}-fixed-value`}
                tooltip="モードが「固定」の場合に、置き換え後の固定文字列を指定します"
                muted={!isFixed}
              >
                固定値:
              </FieldLabel>
              <Input
                id={`${id}-fixed-value`}
                value={values.fixedValue}
                disabled={!isFixed}
                aria-describedby={describedBy(`${id}-fixed-value`)}
                aria-invalid={invalidField === "fixedValue"}
                onChange={(e) => set("fixedValue", e.target.value)}
              />

              <FieldLabel
                htmlFor={`${id}-prefix`}
                tooltip="モードが「連番」の場合に、置き換え後の値の接頭辞を指定します(例: __MASK_TEL_1, __MASK_TEL_2...)"
                muted={isFixed}
              >
                プレフィックス:
              </FieldLabel>
              <Input
                id={`${id}-prefix`}
                value={values.prefix}
                disabled={isFixed}
                aria-describedby={describedBy(`${id}-prefix`)}
                aria-invalid={invalidField === "prefix"}
                onChange={(e) => set("prefix", e.target.value)}
              />

              <FieldLabel
                htmlFor={`${id}-description`}
                tooltip="このルールの用途を書き留めるための自由記述欄です(マスク処理には使われません)"
              >
                説明:
              </FieldLabel>
              <Input
                id={`${id}-description`}
                value={values.description}
                aria-describedby={describedBy(`${id}-description`)}
                onChange={(e) => set("description", e.target.value)}
              />
            </div>

            <div className="flex items-center gap-2">
              <Checkbox
                id={`${id}-enabled`}
                checked={values.enabled}
                aria-describedby={describedBy(`${id}-enabled`)}
                onCheckedChange={(checked) => set("enabled", checked === true)}
              />
              <FieldLabel
                htmlFor={`${id}-enabled`}
                tooltip="オフにすると、このルールを一時的に無効化できます(削除せずに残せます)"
              >
                有効
              </FieldLabel>
            </div>
          </div>

          {errorMessage && <ValidationErrorBox message={errorMessage} />}

          <DialogFooter>
            <Button onClick={onConfirm} disabled={isRequiredFieldMissing}>
              OK
            </Button>
            <Button variant="outline" onClick={() => handleOpenChange(false)}>
              キャンセル
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={pendingAction !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setPendingAction(null);
        }}
        title={
          pendingAction?.kind === "close" ? "変更の破棄確認" : "テンプレートの適用確認"
        }
        description={
          pendingAction?.kind === "close"
            ? "編集中の内容が保存されていません。破棄してもよろしいですか?"
            : "この操作は入力済みの内容を上書きします。続行しますか?"
        }
        onConfirm={confirmPendingAction}
      />
    </TooltipProvider>
  );
}
