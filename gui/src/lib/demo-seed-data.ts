import type { RuleFormValues } from "@/components/rule-edit-dialog";
import type { RuleTemplateOption } from "@/components/rule-edit-dialog";
import type { RuleListItem } from "@/components/rule-edit-screen";

export const RULE_TEMPLATE_OPTIONS: RuleTemplateOption[] = [
  { value: "phone", label: "電話番号(日本)" },
  { value: "ip", label: "IPアドレス" },
  { value: "email", label: "メールアドレス" },
  { value: "password", label: "パスワード(key=value)" },
];

export const RULE_TEMPLATE_VALUES: Record<string, Partial<RuleFormValues>> = {
  phone: {
    name: "電話番号(日本)",
    patternType: "regex",
    pattern: "0\\d{1,4}-\\d{1,4}-\\d{3,4}",
    mode: "sequential",
    prefix: "__MASK_PHONE_",
    description: "日本式電話番号",
  },
  ip: {
    name: "IPアドレス",
    patternType: "regex",
    pattern: "\\b(?:\\d{1,3}\\.){3}\\d{1,3}\\b",
    mode: "sequential",
    prefix: "__MASK_IP_",
    description: "IPv4アドレス",
  },
  email: {
    name: "メールアドレス",
    patternType: "regex",
    pattern: "[\\w.+-]+@[\\w-]+\\.[\\w.-]+",
    mode: "sequential",
    prefix: "__MASK_EMAIL_",
    description: "メールアドレス",
  },
  password: {
    name: "パスワード(key=value)",
    patternType: "regex",
    pattern: "(?i)(password|passwd|pwd)\\s*[:=]\\s*\\S+",
    mode: "fixed",
    fixedValue: "password=__MASK_REDACTED__",
    description: "password=... 形式のkey-value",
  },
};

export const PROFILE_TEMPLATE_RULES: Record<string, Omit<RuleListItem, "id">[]> = {
  general: [
    {
      name: "jp_phone_number",
      patternType: "regex",
      pattern: "0\\d{1,4}-\\d{1,4}-\\d{3,4}",
      mode: "sequential",
      prefix: "__MASK_PHONE_",
      fixedValue: "",
      enabled: true,
      description: "Japanese-style phone numbers",
    },
    {
      name: "ipv4_address",
      patternType: "regex",
      pattern: "\\b(?:\\d{1,3}\\.){3}\\d{1,3}\\b",
      mode: "sequential",
      prefix: "__MASK_IP_",
      fixedValue: "",
      enabled: true,
      description: "IPv4 addresses",
    },
    {
      name: "password_kv",
      patternType: "regex",
      pattern: "(?i)(password|passwd|pwd)\\s*[:=]\\s*\\S+",
      mode: "fixed",
      fixedValue: "password=__MASK_REDACTED__",
      prefix: "",
      enabled: true,
      description: "password=... / passwd: ... key-value pairs",
    },
    {
      name: "email_address",
      patternType: "regex",
      pattern: "[\\w.+-]+@[\\w-]+\\.[\\w.-]+",
      mode: "sequential",
      prefix: "__MASK_EMAIL_",
      fixedValue: "",
      enabled: true,
      description: "Email addresses",
    },
  ],
  sip: [
    {
      name: "sip_uri_phone_user",
      patternType: "regex",
      pattern: "sip:\\d{2,15}@[\\w.-]+",
      mode: "sequential",
      prefix: "__MASK_SIPURI_",
      fixedValue: "",
      enabled: true,
      description: "SIP URIs whose user part is a phone number",
    },
    {
      name: "authorization_header_credentials",
      patternType: "regex",
      pattern: "(?i)Authorization:\\s*Digest\\s+[^\\r\\n]+",
      mode: "fixed",
      fixedValue: "Authorization: Digest __MASK_REDACTED__",
      prefix: "",
      enabled: true,
      description: "SIP Authorization header (Digest credentials)",
    },
    {
      name: "contact_via_header_ip",
      patternType: "regex",
      pattern: "(?i)(Contact|Via):\\s*[^\\r\\n]*?(?:\\d{1,3}\\.){3}\\d{1,3}[^\\r\\n]*",
      mode: "sequential",
      prefix: "__MASK_SIPHDR_",
      fixedValue: "",
      enabled: true,
      description: "Contact/Via headers containing IP addresses",
    },
    {
      name: "jp_phone_number",
      patternType: "regex",
      pattern: "0\\d{1,4}-\\d{1,4}-\\d{3,4}",
      mode: "sequential",
      prefix: "__MASK_PHONE_",
      fixedValue: "",
      enabled: true,
      description: "Japanese-style phone numbers in SIP log bodies",
    },
    {
      name: "ipv4_address",
      patternType: "regex",
      pattern: "\\b(?:\\d{1,3}\\.){3}\\d{1,3}\\b",
      mode: "sequential",
      prefix: "__MASK_IP_",
      fixedValue: "",
      enabled: true,
      description: "Generic IPv4 addresses",
    },
  ],
};

export const DEMO_SAMPLE_TEXT =
  "着信: 0120-000-000\nSIP URI: sip:alice@203.0.113.10\nパスワード: hunter2";
