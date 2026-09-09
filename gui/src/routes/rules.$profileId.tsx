import { useEffect, useRef, useState } from "react";
import { createFileRoute, useNavigate, useRouter } from "@tanstack/react-router";
import { RuleEditScreen, type RuleListItem } from "@/components/rule-edit-screen";
import type { RuleFormValues } from "@/components/rule-edit-dialog";
import { useAppState } from "@/lib/app-state";
import { maskText } from "@/lib/masking-ipc";
import { DEMO_SAMPLE_TEXT, RULE_TEMPLATE_OPTIONS, RULE_TEMPLATE_VALUES } from "@/lib/demo-seed-data";

export const Route = createFileRoute("/rules/$profileId")({ component: RuleEditRoute });

function RuleEditRoute() {
  const { profileId } = Route.useParams();
  const navigate = useNavigate();
  const router = useRouter();
  const appState = useAppState();
  const profileExists = appState.profiles.some((p) => p.id === profileId);

  const [profileName, setProfileName] = useState("");
  const [profileDescription, setProfileDescription] = useState("");
  const [rules, setRules] = useState<RuleListItem[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [sampleText, setSampleText] = useState(DEMO_SAMPLE_TEXT);
  const [maskedResult, setMaskedResult] = useState("");

  // profileが一覧から消えた(削除された)場合のみ戻る。プロファイル一覧全体の変更
  // (無関係な他プロファイルのお気に入り切替等)では再発火しないよう、オブジェクト参照
  // ではなく存在有無のbooleanだけに依存させる(そうしないと編集中の下書きが上書きされうる)。
  useEffect(() => {
    if (!profileExists) navigate({ to: "/profiles" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profileExists]);

  // 詳細(ルール本体)はここでprofileIdが変わった時だけ取得する。profiles一覧の再取得
  // (お気に入り切替等の副作用)に反応して再取得すると、編集中の下書きが失われるため。
  useEffect(() => {
    let cancelled = false;
    appState
      .getProfileDetail(profileId)
      .then((detail) => {
        if (cancelled) return;
        setProfileName(detail.name);
        setProfileDescription(detail.description);
        setRules(detail.rules);
        setLoaded(true);
      })
      .catch((error) => {
        if (cancelled) return;
        console.error("getProfileDetail failed", error);
        navigate({ to: "/profiles" });
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profileId]);

  const profileNameRef = useRef(profileName);
  profileNameRef.current = profileName;

  const isFirstRun = useRef(true);
  useEffect(() => {
    if (!loaded) return;
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
  }, [loaded, profileId, rules, sampleText]);

  if (!profileExists || !loaded) return null;

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
        try {
          await appState.updateProfile(profileId, {
            name: profileName,
            description: profileDescription,
            rules,
          });
          goBack();
        } catch {
          // 失敗の通知はappState.updateProfile内のtoastが行う。
          // ここでは画面遷移を止め、下書きを保持したままにする。
        }
      }}
      onCancel={goBack}
    />
  );
}
