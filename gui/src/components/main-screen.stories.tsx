import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { MainScreen, type ProfileOption } from "./main-screen";

const meta = {
  component: MainScreen,
  parameters: {
    layout: "fullscreen",
  },
} satisfies Meta<typeof MainScreen>;

export default meta;
type Story = StoryObj<typeof meta>;

const PROFILES: ProfileOption[] = [
  { id: "1", name: "SIP監視用" },
  { id: "2", name: "Asterisk本番ログ" },
];

const SAMPLE_TEXT =
  "着信: 0120-000-000\nSIP URI: sip:alice@203.0.113.10\nパスワード: hunter2";

// デモ用の簡易マスク処理(実際のマスキングはRust側masking-coreがIPC経由で行う想定、
// ここでは「マスク実行」クリックでoutputTextが更新されることを示すためだけの簡略版)。
function demoMask(text: string): string {
  return text
    .replace(/0\d{2,4}-\d{2,4}-\d{3,4}/g, "__MASK_TEL_1")
    .replace(/sip:[\w.]+@[\d.]+/g, "__MASK_SIP_1")
    .replace(/hunter2/g, "[REDACTED]");
}

function DemoScreen(props: { initialProfiles: ProfileOption[] }) {
  const [activeProfileId, setActiveProfileId] = useState<string | null>(
    props.initialProfiles[0]?.id ?? null
  );
  const [inputText, setInputText] = useState(SAMPLE_TEXT);
  const [outputText, setOutputText] = useState("");
  const [statusText, setStatusText] = useState("アクティブプロファイル: なし");

  const activeProfile = props.initialProfiles.find((p) => p.id === activeProfileId);

  return (
    <MainScreen
      profiles={props.initialProfiles}
      activeProfileId={activeProfileId}
      onActiveProfileIdChange={setActiveProfileId}
      onOpenProfileList={() => console.log("open profile list")}
      onImport={() => console.log("import")}
      onReload={() => console.log("reload")}
      onNewProfile={() => console.log("new profile")}
      onCreateFromTemplate={() => console.log("create from template")}
      onEditProfile={() => console.log("edit profile")}
      inputText={inputText}
      onInputTextChange={setInputText}
      onLoadFromFile={() => console.log("load from file")}
      onMaskExecute={() => {
        const masked = demoMask(inputText);
        setOutputText(masked);
        setStatusText(
          `アクティブプロファイル: ${activeProfile?.name ?? "なし"} ・ 直近のマスク実行でマッピング3件を置換`
        );
      }}
      onClear={() => setInputText("")}
      outputText={outputText}
      onSaveToFile={() => console.log("save to file")}
      onCopyToClipboard={() => console.log("copy to clipboard")}
      statusText={statusText}
    />
  );
}

export const Default: Story = {
  args: {
    profiles: PROFILES,
    activeProfileId: PROFILES[0].id,
    onActiveProfileIdChange: () => {},
    onOpenProfileList: () => {},
    onImport: () => {},
    onReload: () => {},
    onNewProfile: () => {},
    onCreateFromTemplate: () => {},
    onEditProfile: () => {},
    inputText: SAMPLE_TEXT,
    onInputTextChange: () => {},
    onLoadFromFile: () => {},
    onMaskExecute: () => {},
    onClear: () => {},
    outputText: "",
    onSaveToFile: () => {},
    onCopyToClipboard: () => {},
    statusText: `アクティブプロファイル: ${PROFILES[0].name}`,
  },
  render: () => <DemoScreen initialProfiles={PROFILES} />,
};

export const NoProfiles: Story = {
  args: {
    ...Default.args,
    profiles: [],
    activeProfileId: null,
    statusText: "アクティブプロファイル: なし",
  },
  render: () => <DemoScreen initialProfiles={[]} />,
};
