"""GUI-facing constant data: display labels and quick-fill templates.

Pure data module -- no tkinter dependency, so it can be imported and
tested without a display. app.py builds actual Rule/RuleProfile
instances from the raw dicts here via pydantic (masking_core.models),
keeping validation in one place.
"""

from __future__ import annotations

PATTERN_TYPE_LABELS: dict[str, str] = {
    "literal": "完全一致 (literal)",
    "regex": "正規表現 (regex)",
}
PATTERN_TYPE_VALUES = {v: k for k, v in PATTERN_TYPE_LABELS.items()}

MODE_LABELS: dict[str, str] = {
    "fixed": "固定値 (fixed)",
    "random": "連番 (sequential)",
}
MODE_VALUES = {v: k for k, v in MODE_LABELS.items()}

RULE_TEMPLATES: dict[str, dict[str, str]] = {
    "電話番号(日本)": {
        "pattern_type": "regex",
        "pattern": r"0\d{1,4}-\d{1,4}-\d{3,4}",
        "mode": "random",
        "prefix": "__MASK_PHONE_",
        "description": "日本式電話番号",
    },
    "IPアドレス": {
        "pattern_type": "regex",
        "pattern": r"\b(?:\d{1,3}\.){3}\d{1,3}\b",
        "mode": "random",
        "prefix": "__MASK_IP_",
        "description": "IPv4アドレス",
    },
    "メールアドレス": {
        "pattern_type": "regex",
        "pattern": r"[\w.+-]+@[\w-]+\.[\w.-]+",
        "mode": "random",
        "prefix": "__MASK_EMAIL_",
        "description": "メールアドレス",
    },
    "パスワード(key=value)": {
        "pattern_type": "regex",
        "pattern": r"(?i)(password|passwd|pwd)\s*[:=]\s*\S+",
        "mode": "fixed",
        "fixed_value": "password=__MASK_REDACTED__",
        "description": "password=... 形式のkey-value",
    },
}

PROFILE_TEMPLATES: dict[str, dict] = {
    "汎用 (general)": {
        "profile_name": "general",
        "description": "General-purpose masking rules for arbitrary logs/console output.",
        "rules": [
            {
                "name": "jp_phone_number",
                "pattern_type": "regex",
                "pattern": r"0\d{1,4}-\d{1,4}-\d{3,4}",
                "mode": "random",
                "prefix": "__MASK_PHONE_",
                "description": "Japanese-style phone numbers, e.g. 03-1234-5678 / 090-1234-5678",
            },
            {
                "name": "ipv4_address",
                "pattern_type": "regex",
                "pattern": r"\b(?:\d{1,3}\.){3}\d{1,3}\b",
                "mode": "random",
                "prefix": "__MASK_IP_",
                "description": "IPv4 addresses",
            },
            {
                "name": "password_kv",
                "pattern_type": "regex",
                "pattern": r"(?i)(password|passwd|pwd)\s*[:=]\s*\S+",
                "mode": "fixed",
                "fixed_value": "password=__MASK_REDACTED__",
                "description": "password=... / passwd: ... key-value pairs",
            },
            {
                "name": "email_address",
                "pattern_type": "regex",
                "pattern": r"[\w.+-]+@[\w-]+\.[\w.-]+",
                "mode": "random",
                "prefix": "__MASK_EMAIL_",
                "description": "Email addresses",
            },
        ],
    },
    "SIP": {
        "profile_name": "sip",
        "description": "SIP/FreeSWITCH log masking rules.",
        "rules": [
            {
                "name": "sip_uri_phone_user",
                "pattern_type": "regex",
                "pattern": r"sip:\d{2,15}@[\w.-]+",
                "mode": "random",
                "prefix": "__MASK_SIPURI_",
                "description": "SIP URIs whose user part is a phone number, e.g. sip:0312345678@example.com",
            },
            {
                "name": "authorization_header_credentials",
                "pattern_type": "regex",
                "pattern": r"(?i)Authorization:\s*Digest\s+[^\r\n]+",
                "mode": "fixed",
                "fixed_value": "Authorization: Digest __MASK_REDACTED__",
                "description": "SIP Authorization header (Digest credentials)",
            },
            {
                "name": "contact_via_header_ip",
                "pattern_type": "regex",
                "pattern": r"(?i)(Contact|Via):\s*[^\r\n]*?(?:\d{1,3}\.){3}\d{1,3}[^\r\n]*",
                "mode": "random",
                "prefix": "__MASK_SIPHDR_",
                "description": "Contact/Via headers containing IP addresses (masked as a whole line, before the generic IPv4 rule runs)",
            },
            {
                "name": "jp_phone_number",
                "pattern_type": "regex",
                "pattern": r"0\d{1,4}-\d{1,4}-\d{3,4}",
                "mode": "random",
                "prefix": "__MASK_PHONE_",
                "description": "Japanese-style phone numbers appearing in SIP log bodies",
            },
            {
                "name": "ipv4_address",
                "pattern_type": "regex",
                "pattern": r"\b(?:\d{1,3}\.){3}\d{1,3}\b",
                "mode": "random",
                "prefix": "__MASK_IP_",
                "description": "Generic IPv4 addresses not caught by the header-specific rule above",
            },
        ],
    },
}
