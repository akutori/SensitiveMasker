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
    FIELD_TOOLTIPS,
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


class _ToolTip:
    """ウィジェットにカーソルを合わせたときに説明を表示する簡易tooltip。"""

    def __init__(self, widget: tk.Widget, text: str) -> None:
        self.widget = widget
        self.text = text
        self.tip_window: tk.Toplevel | None = None
        widget.bind("<Enter>", self._show)
        widget.bind("<Leave>", self._hide)
        widget.bind("<Destroy>", self._hide)

    def _show(self, _event: object = None) -> None:
        if self.tip_window is not None or not self.text:
            return
        x = self.widget.winfo_rootx() + 12
        y = self.widget.winfo_rooty() + self.widget.winfo_height() + 4
        self.tip_window = tk.Toplevel(self.widget)
        self.tip_window.wm_overrideredirect(True)
        self.tip_window.wm_geometry(f"+{x}+{y}")
        label = tk.Label(
            self.tip_window,
            text=self.text,
            justify="left",
            background="#ffffe0",
            relief="solid",
            borderwidth=1,
            padx=6,
            pady=3,
            wraplength=320,
        )
        label.pack()

    def _hide(self, _event: object = None) -> None:
        if self.tip_window is not None:
            self.tip_window.destroy()
            self.tip_window = None


class RuleEditDialog(tk.Toplevel):
    """モーダルなルール追加/編集ダイアログ。結果は self.result (Rule | None) に入る。"""

    def __init__(self, parent: tk.Misc, rule: Rule | None = None, title: str | None = None) -> None:
        super().__init__(parent)
        self.title(title or ("ルールを編集" if rule is not None else "ルールを追加"))
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

        template_label = ttk.Label(self, text="テンプレートから入力:")
        template_label.grid(row=row, column=0, sticky="w", **pad)
        _ToolTip(template_label, FIELD_TOOLTIPS["template"])
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

        name_label = ttk.Label(self, text="名前:")
        name_label.grid(row=row, column=0, sticky="w", **pad)
        _ToolTip(name_label, FIELD_TOOLTIPS["name"])
        self.name_var = tk.StringVar(value=rule.name if rule else "")
        ttk.Entry(self, textvariable=self.name_var, width=32).grid(row=row, column=1, sticky="we", **pad)
        row += 1

        pattern_type_label = ttk.Label(self, text="種別:")
        pattern_type_label.grid(row=row, column=0, sticky="w", **pad)
        _ToolTip(pattern_type_label, FIELD_TOOLTIPS["pattern_type"])
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

        pattern_label = ttk.Label(self, text="パターン:")
        pattern_label.grid(row=row, column=0, sticky="w", **pad)
        _ToolTip(pattern_label, FIELD_TOOLTIPS["pattern"])
        self.pattern_var = tk.StringVar(value=rule.pattern if rule else "")
        ttk.Entry(self, textvariable=self.pattern_var, width=32).grid(row=row, column=1, sticky="we", **pad)
        row += 1

        mode_label = ttk.Label(self, text="モード:")
        mode_label.grid(row=row, column=0, sticky="w", **pad)
        _ToolTip(mode_label, FIELD_TOOLTIPS["mode"])
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

        fixed_value_label = ttk.Label(self, text="固定値:")
        fixed_value_label.grid(row=row, column=0, sticky="w", **pad)
        _ToolTip(fixed_value_label, FIELD_TOOLTIPS["fixed_value"])
        self.fixed_value_var = tk.StringVar(value=rule.fixed_value if rule and rule.fixed_value else "")
        self.fixed_value_entry = ttk.Entry(self, textvariable=self.fixed_value_var, width=32)
        self.fixed_value_entry.grid(row=row, column=1, sticky="we", **pad)
        row += 1

        prefix_label = ttk.Label(self, text="プレフィックス:")
        prefix_label.grid(row=row, column=0, sticky="w", **pad)
        _ToolTip(prefix_label, FIELD_TOOLTIPS["prefix"])
        self.prefix_var = tk.StringVar(value=rule.prefix if rule and rule.prefix else "")
        self.prefix_entry = ttk.Entry(self, textvariable=self.prefix_var, width=32)
        self.prefix_entry.grid(row=row, column=1, sticky="we", **pad)
        row += 1

        self.enabled_var = tk.BooleanVar(value=rule.enabled if rule else True)
        enabled_check = ttk.Checkbutton(self, text="有効", variable=self.enabled_var)
        enabled_check.grid(row=row, column=0, columnspan=2, sticky="w", **pad)
        _ToolTip(enabled_check, FIELD_TOOLTIPS["enabled"])
        row += 1

        description_label = ttk.Label(self, text="説明:")
        description_label.grid(row=row, column=0, sticky="w", **pad)
        _ToolTip(description_label, FIELD_TOOLTIPS["description"])
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
        self.geometry("1050x480")
        self.minsize(420, 260)
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

        # side_buttonsを先にside="right"でpackすることで、ウィンドウが縮んでも
        # 常に自分の必要幅を確保できるようにする(アクセシビリティ配慮: ボタンが
        # 隠れて操作不能にならないこと)。幅を固定してpack_propagateを切ることで、
        # ボタン領域自体のサイズも画面サイズによらず一定にする。
        side_buttons = ttk.Frame(body, padding=(4, 0), width=112)
        side_buttons.pack(side="right", fill="y")
        side_buttons.pack_propagate(False)
        ttk.Button(side_buttons, text="追加", width=10, command=self._on_add).pack(fill="x", pady=2)
        ttk.Button(side_buttons, text="コピー", width=10, command=self._on_copy).pack(fill="x", pady=2)
        ttk.Button(side_buttons, text="編集", width=10, command=self._on_edit).pack(fill="x", pady=2)
        ttk.Button(side_buttons, text="削除", width=10, command=self._on_delete).pack(fill="x", pady=2)
        ttk.Button(side_buttons, text="上へ", width=10, command=self._on_move_up).pack(fill="x", pady=(16, 2))
        ttk.Button(side_buttons, text="下へ", width=10, command=self._on_move_down).pack(fill="x", pady=2)

        # ツリー本体はside_buttonsが確保した残り幅を使う。列は常に固定幅
        # (stretch=False)とし、収まりきらない分は横スクロールで見せる
        # (「カラムはそのままに横スクロール」)。縦スクロールも同様に用意する。
        tree_container = ttk.Frame(body)
        tree_container.pack(side="left", fill="both", expand=True)
        tree_container.rowconfigure(0, weight=1)
        tree_container.columnconfigure(0, weight=1)

        columns = ("enabled", "name", "pattern_type", "mode", "pattern", "fixed_value", "prefix", "description")
        self.tree = ttk.Treeview(tree_container, columns=columns, show="headings", selectmode="browse")
        self.tree.heading("enabled", text="有効")
        self.tree.heading("name", text="名前")
        self.tree.heading("pattern_type", text="種別")
        self.tree.heading("mode", text="モード")
        self.tree.heading("pattern", text="パターン")
        self.tree.heading("fixed_value", text="固定値")
        self.tree.heading("prefix", text="プレフィックス")
        self.tree.heading("description", text="説明")
        self.tree.column("enabled", width=40, anchor="center", stretch=False)
        self.tree.column("name", width=90, stretch=False)
        self.tree.column("pattern_type", width=100, stretch=False)
        self.tree.column("mode", width=100, stretch=False)
        self.tree.column("pattern", width=160, stretch=False)
        self.tree.column("fixed_value", width=120, stretch=False)
        self.tree.column("prefix", width=110, stretch=False)
        self.tree.column("description", width=180, stretch=False)

        tree_vscroll = ttk.Scrollbar(tree_container, orient="vertical", command=self.tree.yview)
        tree_hscroll = ttk.Scrollbar(tree_container, orient="horizontal", command=self.tree.xview)
        self.tree.configure(yscrollcommand=tree_vscroll.set, xscrollcommand=tree_hscroll.set)

        self.tree.grid(row=0, column=0, sticky="nsew")
        tree_vscroll.grid(row=0, column=1, sticky="ns")
        tree_hscroll.grid(row=1, column=0, sticky="ew")

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
                    PATTERN_TYPE_LABELS[rule.pattern_type],
                    MODE_LABELS[rule.mode],
                    rule.pattern,
                    rule.fixed_value or "",
                    rule.prefix or "",
                    rule.description or "",
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

    def _on_copy(self) -> None:
        index = self._selected_index()
        if index is None:
            messagebox.showerror("SensitiveMasker", "コピーするルールを選択してください。")
            return
        source = self.rules[index]
        copied = source.model_copy(update={"name": f"{source.name} (コピー)"})
        dialog = RuleEditDialog(self, rule=copied, title="ルールをコピー")
        self.wait_window(dialog)
        if dialog.result is not None:
            self.rules.insert(index + 1, dialog.result)
            self._refresh_tree(select_index=index + 1)

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
        # manage_row(新規作成/テンプレートから作成/プロファイルを編集)には
        # profile_rowのEntryのような伸縮要素がなく、収まりきらない分は隠れて
        # しまう。実測(3ボタン+パディング)で必要な最小幅は約322pxなので、
        # フォント差やDPIスケーリングを見込んだ余裕を持たせて560pxを下限に
        # する(アクセシビリティ配慮: 常にボタンが操作可能であること)。
        self.minsize(560, 400)
        self._apply_icon()

        self.profile: RuleProfile | None = None
        self.mapping_store: MappingStore = MappingStore()
        self._last_focused_text: tk.Text | None = None
        self._search_target: tk.Text | None = None
        self._search_last_index = "1.0"

        self._build_widgets()
        self.bind("<Control-f>", self._on_open_search)

    def _apply_icon(self) -> None:
        icon_path = _icon_path()
        if not icon_path.exists():
            return
        try:
            self.iconbitmap(default=str(icon_path))
        except tk.TclError:
            pass

    def _build_widgets(self) -> None:
        # ボタン類(固定サイズ)を先にpackして必ずスペースを確保し、Entry
        # (fill+expand)を最後にpackして残りを埋めさせる。Tkのpackはコンテナが
        # 縮んだとき「後からpackされたウィジェット」から隠す(サイドの指定に
        # かかわらず)ため、順序で優先度を制御する(アクセシビリティ配慮)。
        profile_row = ttk.Frame(self, padding=8)
        profile_row.pack(fill="x")
        ttk.Label(profile_row, text="プロファイル:").pack(side="left")
        ttk.Button(profile_row, text="再読み込み", command=self._on_reload_profile).pack(side="right", padx=2)
        ttk.Button(profile_row, text="インポート...", command=self._on_import_profile).pack(side="right", padx=2)
        self.profile_path_var = tk.StringVar()
        ttk.Entry(profile_row, textvariable=self.profile_path_var, width=50).pack(
            side="left", padx=4, fill="x", expand=True
        )

        manage_row = ttk.Frame(self, padding=(8, 0, 8, 8))
        manage_row.pack(fill="x")
        ttk.Button(manage_row, text="新規作成...", command=self._on_new_profile).pack(side="left", padx=2)
        ttk.Button(
            manage_row, text="テンプレートから作成...", command=self._on_new_profile_from_template
        ).pack(side="left", padx=2)
        ttk.Button(manage_row, text="プロファイルを編集...", command=self._on_edit_rules).pack(side="left", padx=2)

        # ステータスバーと検索バーはself.panedより先にpackし、ウィンドウが縮んで
        # もTkのpackに隠されないようにする(Tkのpackは後からpackされたウィジェ
        # ットから隠すため、順序で優先度を制御する。アクセシビリティ配慮)。
        # 検索バーはCtrl+Fで動的に表示するが、before=self.status_labelで挿入
        # 位置(=優先度)を固定しておく。
        self.status_var = tk.StringVar(value="プロファイル未読み込み")
        self.status_label = ttk.Label(
            self, textvariable=self.status_var, relief="sunken", anchor="w", padding=4
        )
        self.status_label.pack(fill="x", side="bottom")

        self.search_frame = ttk.Frame(self, padding=(8, 4))
        ttk.Label(self.search_frame, text="検索(Enterで次へ):").pack(side="left")
        self.search_var = tk.StringVar()
        self.search_entry = ttk.Entry(self.search_frame, textvariable=self.search_var, width=40)
        self.search_entry.pack(side="left", padx=4)
        self.search_entry.bind("<Return>", self._on_search_next)
        self.search_entry.bind("<Escape>", self._on_search_close)
        ttk.Button(self.search_frame, text="閉じる", command=self._on_search_close).pack(side="left", padx=2)
        # search_frame is intentionally not packed here -- Ctrl+F shows it

        # ttk.Panedwindowはpane単位のminsizeを持てず、サッシュをドラッグしきると
        # ボタン行が隠れてしまう(pack/gridのminsize指定はコンテナが極端に縮む
        # と効かない場合がある)。classic tk.PanedWindowのminsizeはサッシュの
        # 移動そのものをその位置でクランプしてくれるため、こちらを使う
        # (アクセシビリティ配慮: ドラッグでボタンが操作不能にならないこと)。
        self.paned = tk.PanedWindow(self, orient="vertical", sashwidth=6, sashrelief="raised")
        self.paned.pack(fill="both", expand=True, padx=8, pady=(0, 4))
        self.paned.bind("<Double-Button-1>", self._on_paned_double_click)

        # ボタン行はside="bottom"で先にpackし、テキスト欄はfill="both", expand=True
        # で残りのスペースを埋める。縮小時はテキスト欄側が先に小さくなる。
        top_pane = ttk.Frame(self.paned)
        ttk.Label(top_pane, text="入力テキスト:").pack(anchor="w", side="top")
        button_row = ttk.Frame(top_pane)
        button_row.pack(side="bottom", fill="x")
        ttk.Button(button_row, text="マスク実行 ->", command=self._on_mask_clicked).pack(side="left")
        ttk.Button(button_row, text="クリア", command=self._on_clear_clicked).pack(side="left", padx=4)
        self.input_text = ScrolledText(top_pane, height=12, wrap="word", pady=10)
        self.input_text.pack(fill="both", expand=True, pady=(0, 4))
        self.input_text.tag_config("search_highlight", background="#ffd54f")
        self.input_text.bind("<FocusIn>", lambda e: self._remember_focused_text(self.input_text))
        self.paned.add(top_pane, minsize=90, stretch="always")

        bottom_pane = ttk.Frame(self.paned)
        ttk.Label(bottom_pane, text="出力(マスク後)テキスト:").pack(anchor="w", side="top")
        copy_row = ttk.Frame(bottom_pane)
        copy_row.pack(side="bottom", fill="x")
        ttk.Button(copy_row, text="クリップボードにコピー", command=self._on_copy_clicked).pack(side="right")
        ttk.Button(copy_row, text="ファイルに保存...", command=self._on_save_output_clicked).pack(
            side="right", padx=4
        )
        self.output_text = ScrolledText(bottom_pane, height=12, wrap="word", state="disabled", pady=10)
        self.output_text.pack(fill="both", expand=True, pady=(0, 4))
        self.output_text.tag_config("search_highlight", background="#ffd54f")
        self.output_text.bind("<FocusIn>", lambda e: self._remember_focused_text(self.output_text))
        self.paned.add(bottom_pane, minsize=90, stretch="always")

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
        # 常に新しいMappingStoreで再マッピングする: プロファイル編集直後でも
        # 手動リセット操作なしで変更がそのまま反映される。
        self.mapping_store = MappingStore()
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

    def _on_paned_double_click(self, _event: object = None) -> None:
        self.paned.sash_place(0, 1, self.paned.winfo_height() // 2)

    # --- テキスト検索(Ctrl+F) ------------------------------------------

    def _remember_focused_text(self, widget: tk.Text) -> None:
        self._last_focused_text = widget

    def _on_open_search(self, _event: object = None) -> str:
        new_target = self._last_focused_text or self.input_text
        if self._search_target is not None and self._search_target is not new_target:
            self._search_target.tag_remove("search_highlight", "1.0", "end")
        self._search_target = new_target
        self._search_last_index = "1.0"
        self.search_frame.pack(fill="x", before=self.status_label)
        self.search_entry.focus_set()
        self.search_entry.select_range(0, "end")
        return "break"

    def _on_search_next(self, _event: object = None) -> None:
        if self._search_target is None:
            return
        widget = self._search_target
        query = self.search_var.get()
        widget.tag_remove("search_highlight", "1.0", "end")
        if not query:
            return

        pos = widget.search(query, self._search_last_index, stopindex="end", nocase=True)
        if not pos:
            # テキスト末尾まで見つからなければ先頭から再検索(ラップアラウンド)
            pos = widget.search(query, "1.0", stopindex="end", nocase=True)
        if not pos:
            self._search_last_index = "1.0"
            return

        end_pos = f"{pos}+{len(query)}c"
        widget.tag_add("search_highlight", pos, end_pos)
        widget.see(pos)
        self._search_last_index = end_pos

    def _on_search_close(self, _event: object = None) -> None:
        if self._search_target is not None:
            self._search_target.tag_remove("search_highlight", "1.0", "end")
        self.search_frame.pack_forget()
        if self._search_target is not None:
            self._search_target.focus_set()

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
            filetypes=[
                ("テキストファイル", "*.txt"),
                ("Markdownファイル", "*.md"),
                ("すべてのファイル", "*.*"),
            ],
        )
        if not path:
            return
        masked = self.output_text.get("1.0", "end-1c")
        # .mdはコードブロックで囲んで保存する(ログを複数貼り付けてMarkdown
        # ドキュメントにまとめる際、そのままコードブロックとして使えるように)。
        content = f"```\n{masked}\n```\n" if path.lower().endswith(".md") else masked
        try:
            Path(path).write_text(content, encoding="utf-8")
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
