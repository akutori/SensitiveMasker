from __future__ import annotations

import sys
import tkinter as tk
from pathlib import Path
from tkinter import filedialog, messagebox, ttk
from tkinter.scrolledtext import ScrolledText

from masking_core.masker import MappingStore, apply_profile
from masking_core.models import RuleProfile
from masking_core.profile_io import ProfileLoadError, load_profile


def _default_rules_dir() -> Path:
    if getattr(sys, "frozen", False):
        return Path(sys._MEIPASS) / "rules"  # type: ignore[attr-defined]
    return Path(__file__).resolve().parents[2] / "rules"


class SensitiveMaskerApp(tk.Tk):
    def __init__(self) -> None:
        super().__init__()
        self.title("SensitiveMasker")
        self.geometry("800x600")
        self.minsize(500, 400)

        self.profile: RuleProfile | None = None
        self.mapping_store: MappingStore = MappingStore()

        self._build_widgets()

    def _build_widgets(self) -> None:
        top = ttk.Frame(self, padding=8)
        top.pack(fill="x")

        ttk.Label(top, text="Profile:").pack(side="left")
        self.profile_path_var = tk.StringVar()
        ttk.Entry(top, textvariable=self.profile_path_var, width=50).pack(
            side="left", padx=4, fill="x", expand=True
        )
        ttk.Button(top, text="Browse...", command=self._on_browse_profile).pack(side="left", padx=2)
        ttk.Button(top, text="Reload", command=self._on_reload_profile).pack(side="left", padx=2)

        ttk.Label(self, text="Input text:").pack(anchor="w", padx=8)
        self.input_text = ScrolledText(self, height=12, wrap="word")
        self.input_text.pack(fill="both", expand=True, padx=8, pady=(0, 4))

        button_row = ttk.Frame(self, padding=(8, 4))
        button_row.pack(fill="x")
        ttk.Button(button_row, text="Mask ->", command=self._on_mask_clicked).pack(side="left")
        ttk.Button(button_row, text="Clear", command=self._on_clear_clicked).pack(side="left", padx=4)
        ttk.Button(
            button_row, text="Reset Mapping", command=self._on_reset_mapping_clicked
        ).pack(side="left", padx=4)

        ttk.Label(self, text="Output (masked) text:").pack(anchor="w", padx=8)
        self.output_text = ScrolledText(self, height=12, wrap="word", state="disabled")
        self.output_text.pack(fill="both", expand=True, padx=8, pady=(0, 4))

        copy_row = ttk.Frame(self, padding=(8, 0))
        copy_row.pack(fill="x")
        ttk.Button(copy_row, text="Copy to clipboard", command=self._on_copy_clicked).pack(side="right")

        self.status_var = tk.StringVar(value="No profile loaded")
        ttk.Label(self, textvariable=self.status_var, relief="sunken", anchor="w", padding=4).pack(
            fill="x", side="bottom"
        )

    def _on_browse_profile(self) -> None:
        initial_dir = _default_rules_dir()
        path = filedialog.askopenfilename(
            title="Select a rule profile",
            initialdir=str(initial_dir) if initial_dir.exists() else None,
            filetypes=[("JSON profile", "*.json"), ("All files", "*.*")],
        )
        if not path:
            return
        self.profile_path_var.set(path)
        self._load_profile(path)

    def _on_reload_profile(self) -> None:
        path = self.profile_path_var.get()
        if not path:
            messagebox.showerror("SensitiveMasker", "No profile path set.")
            return
        self._load_profile(path)

    def _load_profile(self, path: str) -> None:
        try:
            self.profile = load_profile(path)
        except ProfileLoadError as exc:
            messagebox.showerror("SensitiveMasker", str(exc))
            return
        self._update_status_bar()

    def _on_mask_clicked(self) -> None:
        if self.profile is None:
            messagebox.showerror("SensitiveMasker", "Load a rule profile first.")
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

    def _update_status_bar(self) -> None:
        if self.profile is None:
            self.status_var.set("No profile loaded")
            return
        self.status_var.set(
            f"Loaded profile '{self.profile.profile_name}' "
            f"({len(self.profile.rules)} rules) | "
            f"Mapping: {len(self.mapping_store.mapping)} entries"
        )


def main() -> None:
    app = SensitiveMaskerApp()
    app.mainloop()


if __name__ == "__main__":
    main()
