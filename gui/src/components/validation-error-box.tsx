import { TriangleAlert } from "lucide-react";

export interface ValidationErrorBoxProps {
  message: string;
}

export function ValidationErrorBox({ message }: ValidationErrorBoxProps) {
  return (
    <div
      role="alert"
      className="flex items-start gap-2 rounded-md border border-border bg-muted p-3 text-sm text-foreground"
    >
      <TriangleAlert className="mt-0.5 size-4 shrink-0" />
      <span>{message}</span>
    </div>
  );
}
