from __future__ import annotations

import sys
import tkinter as tk
from datetime import datetime
from pathlib import Path
from tkinter import filedialog, messagebox, simpledialog, ttk
from tkinter.scrolledtext import ScrolledText

from pydantic import ValidationError

from masking_core.masker import MappingStore, apply_profile
from masking_core.models import Rule, RuleProfile
from masking_core.profile_io import ProfileLoadError, load_profile, save_profile

from gui.settings import (
    MODE_LABELS,
    MODE_VALUES,
    PATTERN_TYPE_LABELS,
    PATTERN_TYPE_VALUES,
    PROFILE_TEMPLATES,
    RULE_TEMPLATES,
)


def _default_rules_dir() -> Path:
    if getattr(sys, "frozen", False):
        return Path(sys._MEIPASS) / "rules"  # type: ignore[attr-defined]
    return Path(__file__).resolve().parents[2] / "rules"


def _icon_path() -> Path:
    if getattr(sys, "frozen", False):
        return Path(sys._MEIPASS) / "assets" / "icon.ico"  # type: ignore[attr-defined]
    return Path(__file__).resolve().parents[2] / "assets" / "icon.ico"


_INVALID_FILENAME_CHARS = '\\/:*?"<>|'


def _safe_filename(name: str) -> str:
    """プロファイル名をWindowsのファイル名として使える形に変換する。"""
    return "".join("_" if c in _INVALID_FILENAME_CHARS else c for c in name).strip() or "profile"


class RuleEditDialog(tk.Toplevel):
    """モーダルなルール追加/編集ダイアログ。結果は self.result (Rule | None) に入る。"""

    def __init__(self, parent: tk.Misc, rule: Rule | None = None) -> None:
        super().__init__(parent)
        self.title("ルールを編集" if rule is not None else "ルールを追加")
        self.resizable(False, False)
        self.result: Rule | None = None

        self._build_widgets(rule)

        self.transient(parent)
        self.grab_set()
        self.protocol("WM_DELETE_WINDOW", self._on_cancel)
        self.wait_visibility()
        self.focus_set()

    def _build_widgets(self, rule: Rule | None) -> None:
        pad = {"padx": 8, "pady": 4}
        row = 0

        ttk.Label(self, text="テンプレートから入力:").grid(row=row, column=0, sticky="w", **pad)
        self.template_var = tk.StringVar(value="(なし)")
        template_combo = ttk.Combobox(
            self,
            textvariable=self.template_var,
            values=["(なし)"] + list(RULE_TEMPLATES.keys()),
            state="readonly",
            width=30,
        )
        template_combo.grid(row=row, column=1, sticky="w", **pad)
        template_combo.bind("<<ComboboxSelected>>", self._on_template_selected)
        row += 1

        ttk.Label(self, text="名前:").grid(row=row, column=0, sticky="w", **pad)
        self.name_var = tk.StringVar(value=rule.name if rule else "")
        ttk.Entry(self, textvariable=self.name_var, width=32).grid(row=row, column=1, sticky="we", **pad)
        row += 1

        ttk.Label(self, text="種別:").grid(row=row, column=0, sticky="w", **pad)
        self.pattern_type_var = tk.StringVar(
            value=PATTERN_TYPE_LABELS[rule.pattern_type if rule else "regex"]
        )
        ttk.Combobox(
            self,
            textvariable=self.pattern_type_var,
            values=list(PATTERN_TYPE_LABELS.values()),
            state="readonly",
            width=30,
        ).grid(row=row, column=1, sticky="w", **pad)
        row += 1

        ttk.Label(self, text="パターン:").grid(row=row, column=0, sticky="w", **pad)
        self.pattern_var = tk.StringVar(value=rule.pattern if rule else "")
        ttk.Entry(self, textvariable=self.pattern_var, width=32).grid(row=row, column=1, sticky="we", **pad)
        row += 1

        ttk.Label(self, text="モード:").grid(row=row, column=0, sticky="w", **pad)
        self.mode_var = tk.StringVar(value=MODE_LABELS[rule.mode if rule else "random"])
        mode_combo = ttk.Combobox(
            self,
            textvariable=self.mode_var,
            values=list(MODE_LABELS.values()),
            state="readonly",
            width=30,
        )
        mode_combo.grid(row=row, column=1, sticky="w", **pad)
        mode_combo.bind("<<ComboboxSelected>>", self._on_mode_changed)
        row += 1

        ttk.Label(self, text="固定値:").grid(row=row, column=0, sticky="w", **pad)
        self.fixed_value_var = tk.StringVar(value=rule.fixed_value if rule and rule.fixed_value else "")
        self.fixed_value_entry = ttk.Entry(self, textvariable=self.fixed_value_var, width=32)
        self.fixed_value_entry.grid(row=row, column=1, sticky="we", **pad)
        row += 1

        ttk.Label(self, text="プレフィックス:").grid(row=row, column=0, sticky="w", **pad)
        self.prefix_var = tk.StringVar(value=rule.prefix if rule and rule.prefix else "")
        self.prefix_entry = ttk.Entry(self, textvariable=self.prefix_var, width=32)
        self.prefix_entry.grid(row=row, column=1, sticky="we", **pad)
        row += 1

        self.enabled_var = tk.BooleanVar(value=rule.enabled if rule else True)
        ttk.Checkbutton(self, text="有効", variable=self.enabled_var).grid(
            row=row, column=0, columnspan=2, sticky="w", **pad
        )
        row += 1

        ttk.Label(self, text="説明:").grid(row=row, column=0, sticky="w", **pad)
        self.description_var = tk.StringVar(value=rule.description if rule and rule.description else "")
        ttk.Entry(self, textvariable=self.description_var, width=32).grid(
            row=row, column=1, sticky="we", **pad
        )
        row += 1

        button_row = ttk.Frame(self)
        button_row.grid(row=row, column=0, columnspan=2, pady=8)
        ttk.Button(button_row, text="OK", command=self._on_ok).pack(side="left", padx=4)
        ttk.Button(button_row, text="キャンセル", command=self._on_cancel).pack(side="left", padx=4)

        self._update_mode_fields_state()

    def _on_template_selected(self, _event: object = None) -> None:
        template = RULE_TEMPLATES.get(self.template_var.get())
        if template is None:
            return
        self.pattern_type_var.set(PATTERN_TYPE_LABELS[template["pattern_type"]])
        self.pattern_var.set(template["pattern"])
        self.mode_var.set(MODE_LABELS[template["mode"]])
        self.fixed_value_var.set(template.get("fixed_value", ""))
        self.prefix_var.set(template.get("prefix", ""))
        self.description_var.set(template.get("description", ""))
        self._update_mode_fields_state()

    def _on_mode_changed(self, _event: object = None) -> None:
        self._update_mode_fields_state()

    def _update_mode_fields_state(self) -> None:
        mode = MODE_VALUES[self.mode_var.get()]
        self.fixed_value_entry.configure(state="normal" if mode == "fixed" else "disabled")
        self.prefix_entry.configure(state="normal" if mode == "random" else "disabled")

    def _on_ok(self) -> None:
        mode = MODE_VALUES[self.mode_var.get()]
        try:
            rule = Rule(
                name=self.name_var.get(),
                pattern_type=PATTERN_TYPE_VALUES[self.pattern_type_var.get()],
                pattern=self.pattern_var.get(),
                mode=mode,
                fixed_value=self.fixed_value_var.get() or None,
                prefix=self.prefix_var.get() or None,
                enabled=self.enabled_var.get(),
                description=self.description_var.get() or None,
            )
        except ValidationError as exc:
            messagebox.showerror("SensitiveMasker", f"入力内容が不正です:\n{exc}")
            return
        self.result = rule
        self.destroy()

    def _on_cancel(self) -> None:
        self.result = None
        self.destroy()


class TemplatePickerDialog(tk.Toplevel):
    """プロファイルのテンプレート(組み込み定義)を1つ選ぶダイアログ。結果は self.result (テンプレート名 | None)。"""

    def __init__(self, parent: tk.Misc) -> None:
        super().__init__(parent)
        self.title("テンプレートを選択")
        self.resizable(False, False)
        self.result: str | None = None

        ttk.Label(self, text="元になるテンプレートを選んでください:").pack(padx=12, pady=(12, 4), anchor="w")

        self.choice_var = tk.StringVar(value=next(iter(PROFILE_TEMPLATES)))
        for label in PROFILE_TEMPLATES:
            ttk.Radiobutton(self, text=label, value=label, variable=self.choice_var).pack(
                padx=24, pady=2, anchor="w"
            )

        button_row = ttk.Frame(self)
        button_row.pack(pady=12)
        ttk.Button(button_row, text="OK", command=self._on_ok).pack(side="left", padx=4)
        ttk.Button(button_row, text="キャンセル", command=self._on_cancel).pack(side="left", padx=4)

        self.transient(parent)
        self.grab_set()
        self.wait_visibility()
        self.focus_set()

    def _on_ok(self) -> None:
        self.result = self.choice_var.get()
        self.destroy()

    def _on_cancel(self) -> None:
        self.result = None
        self.destroy()


class RuleListEditorDialog(tk.Toplevel):
    """プロファイル名・説明・ルール一覧(追加/編集/削除/並び替え)を編集するダイアログ。"""

    def __init__(
        self,
        parent: tk.Misc,
        profile: RuleProfile,
        current_path: str | None,
        on_saved,
    ) -> None:
        super().__init__(parent)
        self.title("プロファイル編集")
        self.geometry("700x450")
        self.current_path = current_path
        self.on_saved = on_saved
        self.rules: list[Rule] = list(profile.rules)

        self._build_widgets(profile)
        self._refresh_tree()

        self.transient(parent)
        self.grab_set()

    def _build_widgets(self, profile: RuleProfile) -> None:
        header = ttk.Frame(self, padding=8)
        header.pack(fill="x")
        ttk.Label(header, text="プロファイル名:").pack(side="left")
        self.profile_name_var = tk.StringVar(value=profile.profile_name)
        ttk.Entry(header, textvariable=self.profile_name_var, width=20).pack(side="left", padx=(2, 12))
        ttk.Label(header, text="説明:").pack(side="left")
        self.description_var = tk.StringVar(value=profile.description or "")
        ttk.Entry(header, textvariable=self.description_var, width=30).pack(
            side="left", padx=2, fill="x", expand=True
        )

        body = ttk.Frame(self, padding=(8, 0))
        body.pack(fill="both", expand=True)

        columns = ("enabled", "name", "pattern_type", "mode", "pattern")
        self.tree = ttk.Treeview(body, columns=columns, show="headings", selectmode="browse")
        self.tree.heading("enabled", text="有効")
        self.tree.heading("name", text="名前")
        self.tree.heading("pattern_type", text="種別")
        self.tree.heading("mode", text="モード")
        self.tree.heading("pattern", text="パターン")
        self.tree.column("enabled", width=40, anchor="center")
        self.tree.column("name", width=100)
        self.tree.column("pattern_type", width=80)
        self.tree.column("mode", width=80)
        self.tree.column("pattern", width=250)
        self.tree.pack(side="left", fill="both", expand=True)

        side_buttons = ttk.Frame(body, padding=(4, 0))
        side_buttons.pack(side="left", fill="y")
        ttk.Button(side_buttons, text="追加", command=self._on_add).pack(fill="x", pady=2)
        ttk.Button(side_buttons, text="編集", command=self._on_edit).pack(fill="x", pady=2)
        ttk.Button(side_buttons, text="削除", command=self._on_delete).pack(fill="x", pady=2)
        ttk.Button(side_buttons, text="上へ", command=self._on_move_up).pack(fill="x", pady=(16, 2))
        ttk.Button(side_buttons, text="下へ", command=self._on_move_down).pack(fill="x", pady=2)

        footer = ttk.Frame(self, padding=8)
        footer.pack(fill="x", side="bottom")
        ttk.Button(footer, text="保存", command=self._on_save).pack(side="right", padx=4)
        ttk.Button(footer, text="キャンセル", command=self._on_cancel).pack(side="right", padx=4)

    def _refresh_tree(self, select_index: int | None = None) -> None:
        self.tree.delete(*self.tree.get_children())
        for i, rule in enumerate(self.rules):
            self.tree.insert(
                "",
                "end",
                iid=str(i),
                values=(
                    "✓" if rule.enabled else "-",
                    rule.name,
                    rule.pattern_type,
                    rule.mode,
                    rule.pattern,
                ),
            )
        if select_index is not None and 0 <= select_index < len(self.rules):
            self.tree.selection_set(str(select_index))

    def _selected_index(self) -> int | None:
        selection = self.tree.selection()
        if not selection:
            return None
        return int(selection[0])

    def _on_add(self) -> None:
        dialog = RuleEditDialog(self)
        self.wait_window(dialog)
        if dialog.result is not None:
            self.rules.append(dialog.result)
            self._refresh_tree(select_index=len(self.rules) - 1)

    def _on_edit(self) -> None:
        index = self._selected_index()
        if index is None:
            messagebox.showerror("SensitiveMasker", "編集するルールを選択してください。")
            return
        dialog = RuleEditDialog(self, rule=self.rules[index])
        self.wait_window(dialog)
        if dialog.result is not None:
            self.rules[index] = dialog.result
            self._refresh_tree(select_index=index)

    def _on_delete(self) -> None:
        index = self._selected_index()
        if index is None:
            messagebox.showerror("SensitiveMasker", "削除するルールを選択してください。")
            return
        if not messagebox.askyesno("SensitiveMasker", f"ルール '{self.rules[index].name}' を削除しますか?"):
            return
        del self.rules[index]
        self._refresh_tree()

    def _on_move_up(self) -> None:
        index = self._selected_index()
        if index is None or index == 0:
            return
        self.rules[index - 1], self.rules[index] = self.rules[index], self.rules[index - 1]
        self._refresh_tree(select_index=index - 1)

    def _on_move_down(self) -> None:
        index = self._selected_index()
        if index is None or index >= len(self.rules) - 1:
            return
        self.rules[index + 1], self.rules[index] = self.rules[index], self.rules[index + 1]
        self._refresh_tree(select_index=index + 1)

    def _on_save(self) -> None:
        name = self.profile_name_var.get().strip()
        if not name:
            messagebox.showerror("SensitiveMasker", "プロファイル名を入力してください。")
            return
        new_profile = RuleProfile(
            profile_name=name,
            description=self.description_var.get() or None,
            rules=self.rules,
        )

        target_path = self.current_path
        if not target_path:
            target_path = filedialog.asksaveasfilename(
                title="プロファイルを保存",
                initialfile=f"{_safe_filename(name)}.json",
                defaultextension=".json",
                filetypes=[("JSONプロファイル", "*.json"), ("すべてのファイル", "*.*")],
            )
            if not target_path:
                return

        try:
            save_profile(new_profile, target_path)
        except ProfileLoadError as exc:
            messagebox.showerror("SensitiveMasker", str(exc))
            return

        self.on_saved(new_profile, target_path)
        self.destroy()

    def _on_cancel(self) -> None:
        self.destroy()


class SensitiveMaskerApp(tk.Tk):
    def __init__(self) -> None:
        super().__init__()
        self.title("SensitiveMasker - 機微情報マスキングツール")
        self.geometry("800x650")
        self.minsize(500, 400)
        self._apply_icon()

        self.profile: RuleProfile | None = None
        self.mapping_store: MappingStore = MappingStore()

        self._build_widgets()

    def _apply_icon(self) -> None:
        icon_path = _icon_path()
        if not icon_path.exists():
            return
        try:
            self.iconbitmap(default=str(icon_path))
        except tk.TclError:
            pass

    def _build_widgets(self) -> None:
        profile_row = ttk.Frame(self, padding=8)
        profile_row.pack(fill="x")
        ttk.Label(profile_row, text="プロファイル:").pack(side="left")
        self.profile_path_var = tk.StringVar()
        ttk.Entry(profile_row, textvariable=self.profile_path_var, width=50).pack(
            side="left", padx=4, fill="x", expand=True
        )
        ttk.Button(profile_row, text="インポート...", command=self._on_import_profile).pack(side="left", padx=2)
        ttk.Button(profile_row, text="再読み込み", command=self._on_reload_profile).pack(side="left", padx=2)

        manage_row = ttk.Frame(self, padding=(8, 0, 8, 8))
        manage_row.pack(fill="x")
        ttk.Button(manage_row, text="新規作成...", command=self._on_new_profile).pack(side="left", padx=2)
        ttk.Button(
            manage_row, text="テンプレートから作成...", command=self._on_new_profile_from_template
        ).pack(side="left", padx=2)
        ttk.Button(manage_row, text="プロファイルを編集...", command=self._on_edit_rules).pack(side="left", padx=2)

        ttk.Label(self, text="入力テキスト:").pack(anchor="w", padx=8)
        self.input_text = ScrolledText(self, height=12, wrap="word")
        self.input_text.pack(fill="both", expand=True, padx=8, pady=(0, 4))

        button_row = ttk.Frame(self, padding=(8, 4))
        button_row.pack(fill="x")
        ttk.Button(button_row, text="マスク実行 ->", command=self._on_mask_clicked).pack(side="left")
        ttk.Button(button_row, text="クリア", command=self._on_clear_clicked).pack(side="left", padx=4)
        ttk.Button(
            button_row, text="マッピングをリセット", command=self._on_reset_mapping_clicked
        ).pack(side="left", padx=4)

        ttk.Label(self, text="出力(マスク後)テキスト:").pack(anchor="w", padx=8)
        self.output_text = ScrolledText(self, height=12, wrap="word", state="disabled")
        self.output_text.pack(fill="both", expand=True, padx=8, pady=(0, 4))

        copy_row = ttk.Frame(self, padding=(8, 0))
        copy_row.pack(fill="x")
        ttk.Button(copy_row, text="クリップボードにコピー", command=self._on_copy_clicked).pack(side="right")
        ttk.Button(copy_row, text="ファイルに保存...", command=self._on_save_output_clicked).pack(
            side="right", padx=4
        )

        self.status_var = tk.StringVar(value="プロファイル未読み込み")
        ttk.Label(self, textvariable=self.status_var, relief="sunken", anchor="w", padding=4).pack(
            fill="x", side="bottom"
        )

    # --- 既存プロファイルの読み込み(インポート/再読み込み) -------------------

    def _on_import_profile(self) -> None:
        initial_dir = _default_rules_dir()
        path = filedialog.askopenfilename(
            title="プロファイルをインポート",
            initialdir=str(initial_dir) if initial_dir.exists() else None,
            filetypes=[("JSONプロファイル", "*.json"), ("すべてのファイル", "*.*")],
        )
        if not path:
            return
        self.profile_path_var.set(path)
        self._load_profile(path)

    def _on_reload_profile(self) -> None:
        path = self.profile_path_var.get()
        if not path:
            messagebox.showerror("SensitiveMasker", "プロファイルのパスが設定されていません。")
            return
        self._load_profile(path)

    def _load_profile(self, path: str) -> None:
        try:
            self.profile = load_profile(path)
        except ProfileLoadError as exc:
            messagebox.showerror("SensitiveMasker", str(exc))
            return
        self._update_status_bar()

    # --- 新規作成/テンプレート作成/プロファイル編集 ---------------------

    def _on_new_profile(self) -> None:
        name = simpledialog.askstring("新規プロファイル", "プロファイル名を入力してください:", parent=self)
        if not name:
            return
        self.profile = RuleProfile(profile_name=name, rules=[])
        self.profile_path_var.set("")
        self._update_status_bar()
        self._on_edit_rules()

    def _on_new_profile_from_template(self) -> None:
        picker = TemplatePickerDialog(self)
        self.wait_window(picker)
        if picker.result is None:
            return
        template = PROFILE_TEMPLATES[picker.result]

        name = simpledialog.askstring(
            "新規プロファイル(テンプレートから)",
            "プロファイル名を入力してください:",
            initialvalue=template["profile_name"],
            parent=self,
        )
        if not name:
            return
        self.profile = RuleProfile(
            profile_name=name,
            description=template["description"],
            rules=[Rule(**rule_kwargs) for rule_kwargs in template["rules"]],
        )
        self.profile_path_var.set("")
        self._update_status_bar()
        self._on_edit_rules()

    def _on_edit_rules(self) -> None:
        if self.profile is None:
            messagebox.showerror("SensitiveMasker", "編集するプロファイルがありません。先に新規作成/インポートしてください。")
            return

        def _on_saved(new_profile: RuleProfile, path: str) -> None:
            self.profile = new_profile
            self.profile_path_var.set(path)
            self._update_status_bar()

        RuleListEditorDialog(self, self.profile, self.profile_path_var.get() or None, _on_saved)

    # --- マスキング操作 ------------------------------------------------

    def _on_mask_clicked(self) -> None:
        if self.profile is None:
            messagebox.showerror("SensitiveMasker", "先にプロファイルを読み込んでください。")
            return
        text = self.input_text.get("1.0", "end-1c")
        masked, self.mapping_store = apply_profile(text, self.profile, self.mapping_store)
        self.output_text.configure(state="normal")
        self.output_text.delete("1.0", "end")
        self.output_text.insert("1.0", masked)
        self.output_text.configure(state="disabled")
        self._update_status_bar()

    def _on_clear_clicked(self) -> None:
        self.input_text.delete("1.0", "end")
        self.output_text.configure(state="normal")
        self.output_text.delete("1.0", "end")
        self.output_text.configure(state="disabled")

    def _on_reset_mapping_clicked(self) -> None:
        self.mapping_store = MappingStore()
        self._update_status_bar()

    def _on_copy_clicked(self) -> None:
        masked = self.output_text.get("1.0", "end-1c")
        self.clipboard_clear()
        self.clipboard_append(masked)
        self.status_var.set("クリップボードにコピーしました")
        self.after(2000, self._update_status_bar)

    def _on_save_output_clicked(self) -> None:
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        path = filedialog.asksaveasfilename(
            title="出力テキストをファイルに保存",
            initialfile=f"output_{timestamp}.txt",
            defaultextension=".txt",
            filetypes=[("テキストファイル", "*.txt"), ("すべてのファイル", "*.*")],
        )
        if not path:
            return
        masked = self.output_text.get("1.0", "end-1c")
        try:
            Path(path).write_text(masked, encoding="utf-8")
        except OSError as exc:
            messagebox.showerror("SensitiveMasker", f"ファイルに保存できません:\n{exc}")

    def _update_status_bar(self) -> None:
        if self.profile is None:
            self.status_var.set("プロファイル未読み込み")
            return
        saved_marker = "" if self.profile_path_var.get() else " [未保存]"
        self.status_var.set(
            f"読み込み済みプロファイル: '{self.profile.profile_name}'{saved_marker} "
            f"({len(self.profile.rules)}件のルール) | "
            f"マッピング: {len(self.mapping_store.mapping)}件"
        )


def main() -> None:
    app = SensitiveMaskerApp()
    app.mainloop()


if __name__ == "__main__":
    main()
