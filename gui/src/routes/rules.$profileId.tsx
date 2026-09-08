import { useEffect, useState } from "react";
import { createFileRoute, useNavigate, useRouter } from "@tanstack/react-router";
import { RuleEditScreen, type RuleListItem } from "@/components/rule-edit-screen";
import type { RuleFormValues } from "@/components/rule-edit-dialog";
import { useAppState } from "@/lib/app-state";
import { simulateMask } from "@/lib/demo-masking";
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

  useEffect(() => {
    if (!profile) navigate({ to: "/profiles" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profile]);

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
      maskedResult={simulateMask(sampleText, rules).text}
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
