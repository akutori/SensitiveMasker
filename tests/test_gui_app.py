"""In-process automated tests for gui/app.py's real tkinter widget behavior.

These instantiate a real SensitiveMaskerApp (a tk.Tk subclass) and drive it
through the same public entry points a user would trigger (typing,
<Return>, button callbacks), then assert on the resulting widget state
(tag_ranges, .get(), StringVar values). No tkinter mocking -- these are
real widgets driven programmatically instead of via mouse/keyboard.

Every code path that could open a real blocking dialog (messagebox.*,
simpledialog.askstring, filedialog.*) is patched so the suite never hangs
waiting for user interaction; see the `patch(...)` calls below.

This machine has a real Tk mainloop available (no virtual display needed),
so no pytest skip-if-no-display guard is used here.

A note on the fixture shape: this machine's AppData\\Roaming (where uv's
managed Python/Tcl install lives) is subject to cloud-sync/reparse-point
style transient file access (OneDrive-managed profile), which made a
*fresh* `tk.Tk()` interpreter -- created and destroyed once per test, ~13
times per run -- intermittently fail with
`TclError: Can't find a usable tk.tcl ... couldn't read file ... scale.tcl`
on a rerun of the full suite roughly 1 time in 5 (confirmed by rerunning
`uv run pytest -q` five times in a row). A single module-scoped
SensitiveMaskerApp (one real Tk interpreter for this whole file, destroyed
once at module teardown so no window leaks) plus an autouse per-test reset
of its mutable state removes that exposure entirely while still exercising
real widgets/real callbacks per test.
"""

from unittest.mock import patch

import pytest

from gui.app import SensitiveMaskerApp
from masking_core.masker import MappingStore
from masking_core.models import Rule, RuleProfile

from tests.fixtures.synthetic_logs import FAKE_PHONE_1


@pytest.fixture(scope="module")
def app():
    """A real, on-screen SensitiveMaskerApp instance, shared by this module
    and destroyed after the last test (see module docstring for why this is
    module-scoped rather than per-test).

    A concrete geometry + update() is required so widget heights are
    actually realized (winfo_height() > 1) before layout-dependent code
    (_apply_bottom_center_padding) runs.
    """
    instance = SensitiveMaskerApp()
    instance.geometry("400x300")
    instance.update()
    try:
        yield instance
    finally:
        instance.destroy()


@pytest.fixture(autouse=True)
def _reset_app_state(app):
    """Restore app to a clean, predictable state before every test.

    Since `app` is shared across the module, tests must not leak state
    (loaded profile, widget text, search position) into one another.
    """
    app.profile = None
    app.mapping_store = MappingStore()
    app.profile_path_var.set("")
    app.input_text.delete("1.0", "end")
    app.output_text.configure(state="normal")
    app.output_text.delete("1.0", "end")
    app.output_text.configure(state="disabled")
    app.input_text.tag_remove("search_highlight", "1.0", "end")
    app.output_text.tag_remove("search_highlight", "1.0", "end")
    app._search_target = None
    app._search_last_index = "1.0"
    app._last_focused_text = None
    app.search_var.set("")
    app.update()
    yield


def _phone_profile() -> RuleProfile:
    """Self-contained test profile (regex phone rule, random/sequential mode)."""
    return RuleProfile(
        profile_name="test",
        rules=[
            Rule(
                name="phone",
                pattern_type="regex",
                pattern=r"0\d{1,4}-\d{1,4}-\d{3,4}",
                mode="random",
                prefix="__MASK_PHONE_",
            )
        ],
    )


# --- _apply_bottom_center_padding regression (bug: zero-width tag range on
#     an empty last line right after pressing Enter) -------------------------


def test_bottom_center_padding_stays_applied_through_enter_and_clear(app):
    widget = app.input_text
    widget.focus_force()
    app.update()

    # (a) after typing text
    widget.insert("1.0", "synthetic log line")
    app.update()
    assert widget.tag_ranges("bottom_center_pad") != ()

    # (b) immediately after pressing Enter, creating a new EMPTY last line --
    # this was the exact bug: the tag range went zero-width and no padding
    # was applied.
    widget.mark_set("insert", "end-1c")
    widget.event_generate("<Return>")
    app.update()
    assert widget.get("1.0", "end") == "synthetic log line\n\n"
    assert widget.tag_ranges("bottom_center_pad") != ()

    # (c) after typing a single character on that new empty line
    widget.insert("insert", "x")
    app.update()
    assert widget.tag_ranges("bottom_center_pad") != ()

    # (d) when the widget is fully cleared to empty
    widget.delete("1.0", "end")
    app.update()
    assert widget.tag_ranges("bottom_center_pad") != ()


def test_bottom_center_padding_applies_to_output_text_after_masking(app):
    app.profile = _phone_profile()
    app.input_text.insert("1.0", f"caller={FAKE_PHONE_1}")
    app.update()

    app._on_mask_clicked()
    app.update()

    assert app.output_text.tag_ranges("bottom_center_pad") != ()


# --- _on_mask_clicked ------------------------------------------------------


def test_on_mask_clicked_masks_output_and_hides_original_value(app):
    app.profile = _phone_profile()
    app.input_text.insert("1.0", f"caller={FAKE_PHONE_1}\n")
    app.update()

    app._on_mask_clicked()
    app.update()

    masked = app.output_text.get("1.0", "end-1c")
    assert "__MASK_PHONE_1__" in masked
    assert FAKE_PHONE_1 not in masked
    # output_text stays read-only (state="disabled") after a mask run.
    assert str(app.output_text.cget("state")) == "disabled"


def test_on_mask_clicked_without_profile_shows_error_and_leaves_output_empty(app):
    assert app.profile is None
    app.input_text.insert("1.0", f"caller={FAKE_PHONE_1}\n")
    app.update()

    with patch("gui.app.messagebox.showerror") as mock_showerror:
        app._on_mask_clicked()
    app.update()

    assert mock_showerror.called
    assert app.output_text.get("1.0", "end-1c") == ""


def test_on_mask_clicked_reuses_same_dummy_for_repeated_value_in_one_run(app):
    # Two occurrences of the SAME original value within a single input must
    # map to the SAME dummy (MappingStore dedup wired correctly by app.py),
    # not two separate counter values.
    app.profile = _phone_profile()
    app.input_text.insert("1.0", f"a={FAKE_PHONE_1} b={FAKE_PHONE_1}\n")
    app.update()

    app._on_mask_clicked()
    app.update()

    masked = app.output_text.get("1.0", "end-1c")
    assert masked.count("__MASK_PHONE_1__") == 2
    assert "__MASK_PHONE_2__" not in masked


def test_on_mask_clicked_resets_mapping_each_run_so_repeat_clicks_are_stable(app):
    # app._on_mask_clicked builds a *new* MappingStore on every click (see
    # the comment in app.py), so clicking "mask" again on unchanged input
    # must reproduce the exact same output, not advance the counter to _2__.
    app.profile = _phone_profile()
    app.input_text.insert("1.0", f"caller={FAKE_PHONE_1}\n")
    app.update()

    app._on_mask_clicked()
    app.update()
    first_run = app.output_text.get("1.0", "end-1c")

    app._on_mask_clicked()
    app.update()
    second_run = app.output_text.get("1.0", "end-1c")

    assert first_run == second_run == "caller=__MASK_PHONE_1__\n"


# --- _on_clear_clicked -------------------------------------------------


def test_on_clear_clicked_empties_input_and_output(app):
    app.profile = _phone_profile()
    app.input_text.insert("1.0", f"caller={FAKE_PHONE_1}\n")
    app.update()
    app._on_mask_clicked()
    app.update()
    assert app.output_text.get("1.0", "end-1c") != ""

    app._on_clear_clicked()
    app.update()

    assert app.input_text.get("1.0", "end-1c") == ""
    assert app.output_text.get("1.0", "end-1c") == ""
    assert str(app.output_text.cget("state")) == "disabled"


# --- _load_profile / _on_reload_profile error path ----------------------


def test_reload_profile_with_invalid_path_shows_error_without_real_dialog_and_keeps_old_profile(
    app, tmp_path
):
    original_profile = RuleProfile(profile_name="original_untouched", rules=[])
    app.profile = original_profile
    missing_path = tmp_path / "does_not_exist.json"
    app.profile_path_var.set(str(missing_path))

    with patch("gui.app.messagebox.showerror") as mock_showerror:
        app._on_reload_profile()
    app.update()

    assert mock_showerror.called
    title, _message = mock_showerror.call_args[0]
    assert title == "SensitiveMasker"
    # Previous profile is preserved on load failure.
    assert app.profile is original_profile


def test_reload_profile_with_malformed_json_shows_error_and_keeps_old_profile(app, tmp_path):
    original_profile = RuleProfile(profile_name="original_untouched", rules=[])
    app.profile = original_profile
    bad_path = tmp_path / "broken.json"
    bad_path.write_text("{not valid json", encoding="utf-8")
    app.profile_path_var.set(str(bad_path))

    with patch("gui.app.messagebox.showerror") as mock_showerror:
        app._on_reload_profile()
    app.update()

    assert mock_showerror.called
    assert app.profile is original_profile


def test_reload_profile_with_empty_path_shows_error_without_real_dialog(app):
    assert app.profile_path_var.get() == ""

    with patch("gui.app.messagebox.showerror") as mock_showerror:
        app._on_reload_profile()
    app.update()

    assert mock_showerror.called


# --- _on_new_profile -----------------------------------------------------


def test_on_new_profile_builds_empty_profile_and_clears_saved_path(app):
    app.profile_path_var.set("some/previously/loaded/profile.json")

    # simpledialog.askstring would otherwise block on a real modal prompt;
    # _on_edit_rules would otherwise open a real modal RuleListEditorDialog.
    with patch("gui.app.simpledialog.askstring", return_value="synthetic_new_profile"):
        with patch.object(app, "_on_edit_rules") as mock_edit_rules:
            app._on_new_profile()
    app.update()

    assert app.profile is not None
    assert app.profile.profile_name == "synthetic_new_profile"
    assert app.profile.rules == []
    # Starting a brand-new profile clears any previously loaded file path.
    assert app.profile_path_var.get() == ""
    assert mock_edit_rules.called


def test_on_new_profile_cancelled_leaves_profile_unset(app):
    assert app.profile is None
    with patch("gui.app.simpledialog.askstring", return_value=None):
        app._on_new_profile()
    app.update()

    assert app.profile is None


# --- search (Ctrl+F) -----------------------------------------------------


def test_search_next_highlights_each_match_and_wraps_around(app):
    app.input_text.insert("1.0", "alpha needle beta needle gamma")
    app.update()
    app._search_target = app.input_text
    app.search_var.set("needle")

    app._on_search_next()
    app.update()
    first_ranges = app.input_text.tag_ranges("search_highlight")
    assert [str(idx) for idx in first_ranges] == ["1.6", "1.12"]

    app._on_search_next()
    app.update()
    second_ranges = app.input_text.tag_ranges("search_highlight")
    assert [str(idx) for idx in second_ranges] == ["1.18", "1.24"]

    # A third search past the last match wraps back around to the first.
    app._on_search_next()
    app.update()
    wrapped_ranges = app.input_text.tag_ranges("search_highlight")
    assert [str(idx) for idx in wrapped_ranges] == [str(idx) for idx in first_ranges]
