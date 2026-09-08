import Editor from "@monaco-editor/react";
import "@/lib/monaco-setup";

export interface MaskedTextEditorProps {
  value: string;
  onChange?: (value: string) => void;
  readOnly?: boolean;
  ariaLabel: string;
}

export function MaskedTextEditor({
  value,
  onChange,
  readOnly = false,
  ariaLabel,
}: MaskedTextEditorProps) {
  return (
    <div className="overflow-hidden rounded-lg border border-input focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50">
      <Editor
        height="170px"
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
