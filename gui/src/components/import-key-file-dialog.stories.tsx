import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, userEvent, waitFor } from "storybook/test";
import { ImportKeyFileDialog } from "./import-key-file-dialog";

const meta = {
  component: ImportKeyFileDialog,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof ImportKeyFileDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

const FILE_NAME = "profiles_export_20260901.smx";

// 鍵ファイルを選んでいない: 選択を促す。OKは押せない。
export const NoKeyFileSelected: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    fileName: FILE_NAME,
    keyFileName: null,
    dragActive: false,
    onSelectKeyFile: fn(),
    onConfirm: fn(),
  },
  play: async ({ args }) => {
    await waitFor(() => expect(screen.getByText(`選択したファイル: ${FILE_NAME}`)).toBeVisible());
    await waitFor(() => expect(screen.getByText("鍵ファイルが選択されていません")).toBeVisible());
    await expect(screen.getByRole("button", { name: "OK" })).toBeDisabled();

    await userEvent.click(screen.getByRole("button", { name: "鍵ファイルを選択…" }));
    await expect(args.onSelectKeyFile).toHaveBeenCalledTimes(1);
  },
};

// 鍵ファイルを選んだ: 名前を示し、OKを押せる。
export const KeyFileSelected: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    fileName: FILE_NAME,
    keyFileName: "profiles_export.smxkey",
    dragActive: false,
    onSelectKeyFile: fn(),
    onConfirm: fn(),
  },
  play: async ({ args }) => {
    await waitFor(() => expect(screen.getByText("profiles_export.smxkey")).toBeVisible());
    await expect(screen.getByRole("button", { name: "OK" })).toBeEnabled();

    await userEvent.click(screen.getByRole("button", { name: "OK" }));
    await expect(args.onConfirm).toHaveBeenCalledTimes(1);
  },
};

// ファイルをドラッグして、ウィンドウの上に来ている間は、ドロップの受け皿を強調し、ドロップを促す。
export const DragActive: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    fileName: FILE_NAME,
    keyFileName: null,
    dragActive: true,
    onSelectKeyFile: fn(),
    onConfirm: fn(),
  },
  play: async () => {
    const hint = await screen.findByText("ここへドロップしてください");
    await waitFor(() => expect(hint).toBeVisible());
    await expect(hint.parentElement).toHaveAttribute("data-drag-active", "true");
    await expect(screen.queryByText("鍵ファイルが選択されていません")).toBeNull();
  },
};

// 復号に失敗した(別の鍵ファイル・拡張子が違うなど): 理由を示す。
export const WithError: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    fileName: FILE_NAME,
    keyFileName: "other.smxkey",
    dragActive: false,
    onSelectKeyFile: fn(),
    errorMessage: "この鍵ファイルでは復号できません(エクスポート時に保存した、別の鍵ファイルを指定してください)",
    onConfirm: fn(),
  },
  play: async () => {
    await expect(await screen.findByRole("alert")).toHaveTextContent("この鍵ファイルでは復号できません");
  },
};

// 復号している間は、OKも「鍵ファイルを選択」も押せず、その理由を示す。閉じることはできる。
export const Busy: Story = {
  args: {
    open: true,
    onOpenChange: fn(),
    fileName: FILE_NAME,
    keyFileName: "profiles_export.smxkey",
    dragActive: false,
    onSelectKeyFile: fn(),
    onConfirm: fn(),
    busy: true,
  },
  play: async ({ args }) => {
    await expect(await screen.findByRole("status")).toHaveTextContent("復号しています…");
    await expect(screen.getByRole("button", { name: "OK" })).toBeDisabled();
    await expect(screen.getByRole("button", { name: "鍵ファイルを選択…" })).toBeDisabled();
    await expect(screen.getByRole("button", { name: "キャンセル" })).toBeEnabled();

    await userEvent.click(screen.getByRole("button", { name: "キャンセル" }));
    await expect(args.onOpenChange).toHaveBeenLastCalledWith(false);
  },
};
