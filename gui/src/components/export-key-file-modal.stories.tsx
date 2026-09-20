import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, userEvent, waitFor } from "storybook/test";
import { ExportKeyFileModal } from "./export-key-file-modal";

const meta = {
  component: ExportKeyFileModal,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ExportKeyFileModal>;

export default meta;
type Story = StoryObj<typeof meta>;

// 背景をクリックする操作(ダイアログの外側)。
function clickOverlay() {
  const overlay = document.querySelector<HTMLElement>('[data-slot="dialog-overlay"]');
  if (!overlay) throw new Error("ダイアログの背景が見つからない");
  return userEvent.click(overlay);
}

// 何も始めていない: 紛失の注意が出て、エクスポートできる。
export const Idle: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    target: "SIP監視用",
    phase: "idle",
    onExport: fn(),
  },
  play: async ({ args }) => {
    await waitFor(() => expect(screen.getByText("この鍵ファイルを紛失すると、このプロファイルは二度と復号できません。")).toBeVisible());
    await expect(screen.getByRole("button", { name: "エクスポート" })).toBeEnabled();
    await expect(screen.getByRole("button", { name: "キャンセル" })).toBeEnabled();

    await userEvent.click(screen.getByRole("button", { name: "エクスポート" }));
    await expect(args.onExport).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "キャンセル" }));
    await expect(args.onOpenChange).toHaveBeenLastCalledWith(false);
  },
};

// 保存先を選んでいる間は、まだ何も書き出していないため、閉じられる。エクスポートは、押せない。
export const ChoosingKeyFile: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    target: "SIP監視用",
    phase: "choosingKeyFile",
    onExport: fn(),
  },
  play: async ({ args }) => {
    await waitFor(() => expect(screen.getByText("鍵ファイルの保存先を選択しています…")).toBeVisible());
    await expect(screen.getByRole("button", { name: "エクスポート" })).toBeDisabled();
    await expect(screen.getByRole("button", { name: "キャンセル" })).toBeEnabled();

    await userEvent.keyboard("{Escape}");
    await expect(args.onOpenChange).toHaveBeenLastCalledWith(false);
  },
};

export const ChoosingExportFile: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    target: "SIP監視用",
    phase: "choosingExportFile",
    onExport: fn(),
  },
  play: async () => {
    await waitFor(() => expect(screen.getByText("エクスポートするファイルの保存先を選択しています…")).toBeVisible());
    await expect(screen.getByRole("button", { name: "エクスポート" })).toBeDisabled();
    await expect(screen.getByRole("button", { name: "キャンセル" })).toBeEnabled();
  },
};

// 書き込み中は、閉じる操作(×・Escape・背景・キャンセル)を受け付けない。
export const Writing: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    target: "SIP監視用",
    phase: "writing",
    onExport: fn(),
  },
  play: async ({ args }) => {
    await waitFor(() => expect(screen.getByText("書き込んでいます…")).toBeVisible());
    await expect(screen.getByRole("button", { name: "エクスポート" })).toBeDisabled();
    await expect(screen.getByRole("button", { name: "キャンセル" })).toBeDisabled();
    await expect(screen.queryByRole("button", { name: "Close" })).toBeNull();

    await userEvent.keyboard("{Escape}");
    await clickOverlay();
    await expect(args.onOpenChange).not.toHaveBeenCalled();
  },
};
