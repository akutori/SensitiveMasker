import Editor from "@monaco-editor/react";
import { cn } from "cn";
import "@/lib/monaco-setup";

export interface MaskedTextEditorProps {
  value: string;
  onChange?: (value: string) => void;
  readOnly?: boolean;
  ariaLabel: string;
  className?: string;
  height?: string;
}

export function MaskedTextEditor({
  value,
  onChange,
  readOnly = false,
  ariaLabel,
  className,
  height = "170px",
}: MaskedTextEditorProps) {
  return (
    <div
      className={cn(
        "overflow-hidden rounded-lg border border-input focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50",
        className
      )}
    >
      <Editor
        height={height}
        language="plaintext"
        value={value}
        onChange={(next) => onChange?.(next ?? "")}
        theme={readOnly ? "sensitivemasker-output" : "sensitivemasker-input"}
        loading={
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            読み込み中...
          </div>
        }
        options={{
          readOnly,
          minimap: { enabled: false },
          wordWrap: "on",
          scrollBeyondLastLine: false,
          fontSize: 13,
          lineNumbers: "on",
          renderLineHighlight: readOnly ? "none" : "line",
          automaticLayout: true,
          ariaLabel,
        }}
      />
    </div>
  );
}
