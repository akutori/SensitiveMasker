import { useEffect, useRef, useState } from "react";
import { createFileRoute, useNavigate, useRouter } from "@tanstack/react-router";
import { RuleEditScreen, type RuleListItem } from "@/components/rule-edit-screen";
import type { RuleFormValues } from "@/components/rule-edit-dialog";
import { useAppState } from "@/lib/app-state";
import { maskText } from "@/lib/masking-ipc";
import { DEMO_SAMPLE_TEXT, RULE_TEMPLATE_OPTIONS, RULE_TEMPLATE_VALUES } from "@/lib/demo-seed-data";

export const Route = createFileRoute("/rules/$profileId")({
  component: RuleEditRoute,
});

function RuleEditRoute() {
  const { profileId } = Route.useParams();
  const navigate = useNavigate();
  const router = useRouter();
  const appState = useAppState();

  const profile = appState.profiles.find((p) => p.id === profileId);

  const [profileName, setProfileName] = useState(profile?.name ?? "");
  const [profileDescription, setProfileDescription] = useState(profile?.description ?? "");
  const [rules, setRules] = useState<RuleListItem[]>(appState.rulesByProfileId[profileId] ?? []);
  const [sampleText, setSampleText] = useState(DEMO_SAMPLE_TEXT);
  const [maskedResult, setMaskedResult] = useState("");

  useEffect(() => {
    if (!profile) navigate({ to: "/profiles" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profile]);

  // profileNameは変更のたびにプレビューを再計算する必要が無い(masking-coreの
  // マスク処理自体はプロファイル名を使わない)ため、依存配列には含めずrefで最新値だけ読む。
  const profileNameRef = useRef(profileName);
  profileNameRef.current = profileName;

  // 入力のたびにIPCを呼ばないよう、最後の変更から一定時間待ってから実行する
  // (初回表示時だけは遅延させず即座に計算する)。連続入力中に古い呼び出しの結果が
  // 後から返って上書きすることを防ぐため、クリーンアップ時点でそのエフェクトの結果を破棄する。
  // isFirstRunは成功時にのみfalseへ倒す(React 18 StrictModeの開発時二重実行で
  // 1回目がキャンセルされても、2回目が引き続き「初回」として即時実行されるようにするため)。
  const isFirstRun = useRef(true);
  useEffect(() => {
    let cancelled = false;
    const run = () => {
      maskText(profileId, profileNameRef.current, rules, sampleText)
        .then((result) => {
          if (cancelled) return;
          isFirstRun.current = false;
          setMaskedResult(result.text);
        })
        .catch((error) => {
          if (cancelled) return;
          console.error("mask_text failed", error);
          setMaskedResult("(プレビューを計算できませんでした)");
        });
    };
    if (isFirstRun.current) {
      run();
      return () => {
        cancelled = true;
      };
    }
    const timer = setTimeout(run, 300);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [profileId, rules, sampleText]);

  if (!profile) return null;

  const goBack = () => {
    if (window.history.length > 1) router.history.back();
    else navigate({ to: "/profiles" });
  };

  return (
    <RuleEditScreen
      profileName={profileName}
      onProfileNameChange={setProfileName}
      profileDescription={profileDescription}
      onProfileDescriptionChange={setProfileDescription}
      rules={rules}
      onReorderRules={setRules}
      onToggleRuleEnabled={(id) =>
        setRules((prev) => prev.map((r) => (r.id === id ? { ...r, enabled: !r.enabled } : r)))
      }
      onAddRule={(values: RuleFormValues) =>
        setRules((prev) => [...prev, { ...values, id: crypto.randomUUID() }])
      }
      onEditRule={(id, values) =>
        setRules((prev) => prev.map((r) => (r.id === id ? { ...values, id } : r)))
      }
      onDeleteRule={(id) => setRules((prev) => prev.filter((r) => r.id !== id))}
      ruleTemplateOptions={RULE_TEMPLATE_OPTIONS}
      onResolveRuleTemplate={(value) => RULE_TEMPLATE_VALUES[value] ?? {}}
      sampleText={sampleText}
      onSampleTextChange={setSampleText}
      maskedResult={maskedResult}
      onSave={async () => {
        await appState.updateProfileMeta(profileId, {
          name: profileName,
          description: profileDescription,
        });
        await appState.saveRules(profileId, rules);
        goBack();
      }}
      onCancel={goBack}
    />
  );
}
