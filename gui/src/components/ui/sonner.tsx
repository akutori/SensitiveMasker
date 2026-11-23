import { Toaster as Sonner, type ToasterProps } from "sonner"
import { CircleCheckIcon, InfoIcon, TriangleAlertIcon, OctagonXIcon, Loader2Icon } from "lucide-react"

// このプロジェクトはnext-themesを使っていないため、システム設定に委ねる
// theme="system"を直接指定する(next-themesのuseThemeは導入しない)。
const Toaster = ({ ...props }: ToasterProps) => {
  return (
    <Sonner
      theme="system"
      className="toaster group"
      icons={{
        success: (
          <CircleCheckIcon className="size-4" />
        ),
        info: (
          <InfoIcon className="size-4" />
        ),
        warning: (
          <TriangleAlertIcon className="size-4" />
        ),
        error: (
          <OctagonXIcon className="size-4" />
        ),
        loading: (
          <Loader2Icon className="size-4 animate-spin" />
        ),
      }}
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-text": "var(--popover-foreground)",
          "--normal-border": "var(--border)",
          "--border-radius": "var(--radius)",
          // 閉じるボタンを、既定の左上でなく、右上に置く(文章の向きを変える dir="rtl" を使わずに済ませるため、
          // sonnerのrtl用と同じ値を直接指定する)。
          "--toast-close-button-start": "unset",
          "--toast-close-button-end": "0",
          "--toast-close-button-transform": "translate(35%, -35%)",
        } as React.CSSProperties
      }
      toastOptions={{
        // 閉じるボタンの読み上げ名(既定は英語の"Close toast")。
        closeButtonAriaLabel: "通知を閉じる",
        classNames: {
          toast: "cn-toast",
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
