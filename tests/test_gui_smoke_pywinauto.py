"""Black-box smoke test for the tkinter GUI (src/gui/app.py) via pywinauto.

This drives the app the way an OS-level UI automation tool (or a
screen-reader) would: it launches ``python -m gui.app`` as a REAL, separate
OS process and talks to its REAL window through Windows UI Automation
(pywinauto's "uia" backend) -- there is no in-process widget access here.
The in-process/unit-style GUI tests live in ``tests/test_gui_app.py`` (owned
by a different part of the test suite); this file is a *separate* tier and
must stay independent of it.

All tests here are marked ``@pytest.mark.pywinauto`` and are excluded from
the default ``uv run pytest`` run (see ``addopts`` in pyproject.toml) because
they are slow and open a real OS window. Run them explicitly with::

    uv run pytest tests/test_gui_smoke_pywinauto.py -v -m pywinauto

IMPORTANT, verified limitation of this test tier
--------------------------------------------------
tkinter/ttk widgets on Windows expose essentially NO OS accessibility
information. This was confirmed directly against this app's real running
window before writing this test:

* ``win32gui.EnumChildWindows()`` on the main window's HWND returns child
  windows that are ALL of the generic class ``"TkChild"``, and NONE of them
  has any window text (``GetWindowText()`` is empty for every one) -- so the
  classic pywinauto ``backend="win32"`` (which matches controls by native
  window text/class) cannot see button captions at all.
* Connecting with ``backend="uia"`` and walking ``window.descendants()``
  shows every ttk widget (frames, buttons, labels, the entry, ...) surfacing
  as a generic, nameless ``"Pane"`` element: no ``Name``, no
  ``AutomationId``, and no ``LegacyIAccessible.Name`` either. Only genuinely
  native Win32 pieces (the title bar's minimize/maximize/close buttons, and
  the ``ScrolledText`` widgets' scrollbars) come through with a real
  control_type/name.

In short: **it is not possible to look up "マスク実行 ->" or "クリア" by
their label text through pywinauto/UI Automation for this app** -- there is
nothing in the OS accessibility tree that carries a ttk widget's text. This
is a well-known limitation of raw tkinter on Windows (the same reason screen
readers such as NVDA/JAWS have historically struggled with tkinter apps), not
a bug in this test file or a wrong backend choice. "uia" is still the
better backend to use here (as suggested), since it is the only one that
gives us a normal window handle + wait('visible') + click_input() workflow;
"win32" sees literally nothing more useful for this app's own widgets.

Given that, this smoke test verifies the two known buttons the way a
sighted, mouse-only user effectively would, and the way any black-box tool
would have to for this app:

1. It finds the window by its exact title (this DOES work -- the toplevel's
   window text is real native window text).
2. It locates the button row by structural position: per
   ``SensitiveMaskerApp._build_widgets`` (src/gui/app.py), "マスク実行 ->"
   and "クリア" are packed left-to-right, side by side, in a dedicated row
   directly above the input ScrolledText box -- and that row is the
   *topmost* (smallest ``top``) of the two "two small side-by-side leaf
   panes" rows in the window (the other one is the output pane's
   "クリップボードにコピー" / "ファイルに保存..." row, further down). This
   is a relative/structural match, not a hard-coded absolute pixel
   coordinate, so it survives the window opening at whatever screen position
   the OS happens to pick.
3. It clicks the left-hand (first) button -- "マスク実行 ->" -- with no
   profile loaded, which really only proves the button exists and is wired
   up if a real, observable, native side effect follows. It does: the app's
   own ``_on_mask_clicked`` calls ``messagebox.showerror(...)``, which IS a
   genuine native Win32 dialog (class ``#32770``) with a real, readable
   "OK" button and real, readable message text -- unlike the app's own
   widgets. The test asserts this dialog appears (by class + owner HWND,
   confirmed live against this machine) and closes it via its real "OK"
   button.
4. It clicks the right-hand (second) button -- "クリア" -- and asserts the
   app is still alive and responsive afterwards (main window still present,
   title unchanged, no crash/hang), which is the strongest existence+wiring
   proof available for a button whose own click has no separately observable
   native side effect when the app is in its initial empty state.

The whole process is torn down in a ``finally`` block so no orphan window or
process survives a failed assertion.
"""

from __future__ import annotations

import subprocess
import sys
import time
from pathlib import Path

import pytest

pywinauto = pytest.importorskip("pywinauto")

from pywinauto.application import Application  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[1]
WINDOW_TITLE = "SensitiveMasker - 機微情報マスキングツール"
LAUNCH_TIMEOUT = 20
DIALOG_TIMEOUT = 10


def _launch_gui_process() -> subprocess.Popen:
    """Launch the GUI the same way a user/README would: as a real subprocess.

    ``gui`` is importable without extra PYTHONPATH wrangling because the
    project is installed (editable) into the active environment by
    ``uv sync`` (see pyproject.toml's ``[tool.hatch.build.targets.wheel]``
    package mapping) -- so plain ``sys.executable -m gui.app`` run from the
    repo root works, matching how ``uv run python -m gui.app`` is documented
    in CLAUDE.md.
    """
    return subprocess.Popen(
        [sys.executable, "-m", "gui.app"],
        cwd=str(REPO_ROOT),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def _find_button_row(win) -> list:
    """Return the [left_button, right_button] elements of the button row
    directly above the input text box ("マスク実行 ->", "クリア"), located
    structurally rather than by text (see module docstring for why text
    matching is not possible for this app).
    """
    panes = [d for d in win.descendants() if d.element_info.control_type == "Pane"]

    candidate_rows = []
    for pane in panes:
        children = [c for c in pane.children() if c.element_info.control_type == "Pane"]
        if len(children) != 2:
            continue
        is_leaf_pair = all(len(c.children()) == 0 for c in children)
        is_button_sized = all(
            c.rectangle().height() < 40 and c.rectangle().width() < 150 for c in children
        )
        if is_leaf_pair and is_button_sized:
            candidate_rows.append((pane, sorted(children, key=lambda c: c.rectangle().left)))

    assert len(candidate_rows) >= 2, (
        "expected at least 2 button-shaped rows (mask/clear row + copy/save "
        f"row), found {len(candidate_rows)} -- the window layout may have "
        "changed; update the structural heuristic in _find_button_row."
    )

    # The mask/clear row sits above (smaller "top") the copy/save row.
    candidate_rows.sort(key=lambda item: item[0].rectangle().top)
    _row_pane, buttons = candidate_rows[0]
    assert len(buttons) == 2
    return buttons


@pytest.mark.pywinauto
def test_gui_launches_shows_main_window_and_has_mask_and_clear_buttons():
    proc = _launch_gui_process()
    app = None
    try:
        # -- connect to the real OS window (real UI Automation, not in-process) --
        app = Application(backend="uia").connect(title=WINDOW_TITLE, timeout=LAUNCH_TIMEOUT)
        win = app.window(title=WINDOW_TITLE)
        win.wait("visible", timeout=LAUNCH_TIMEOUT)

        assert win.window_text() == WINDOW_TITLE

        mask_button, clear_button = _find_button_row(win)

        # -- "マスク実行 ->": click it (no profile loaded) and require the
        # real native error dialog it triggers, proving the button exists
        # and is wired to its real click handler.
        win.set_focus()
        mask_button.click_input()

        error_dialog = None
        try:
            dialog_app = Application(backend="win32").connect(
                title="SensitiveMasker", class_name="#32770", timeout=DIALOG_TIMEOUT
            )
            error_dialog = dialog_app.window(title="SensitiveMasker", class_name="#32770")
            error_dialog.wait("visible", timeout=DIALOG_TIMEOUT)
        except Exception:
            error_dialog = None

        assert error_dialog is not None, (
            "expected clicking the mask-execute button (no profile loaded) "
            "to raise a native 'SensitiveMasker' error messagebox -- the "
            "button may not exist at the expected position, or the click "
            "did not reach it"
        )
        ok_button = error_dialog.child_window(title="OK", class_name="Button")
        assert ok_button.exists(timeout=DIALOG_TIMEOUT)
        ok_button.click_input()
        # dialog must actually close, or later assertions on the main
        # window would be unreliable
        error_dialog.wait_not("visible", timeout=DIALOG_TIMEOUT)

        # -- "クリア": click it and require the app to still be alive and
        # responsive afterwards -- the strongest available existence proof
        # for this button given it produces no other observable native
        # side effect from the initial empty state.
        win.set_focus()
        clear_button.click_input()
        time.sleep(0.3)
        assert win.exists(timeout=DIALOG_TIMEOUT)
        assert win.window_text() == WINDOW_TITLE
    finally:
        if app is not None:
            try:
                main_win = app.window(title=WINDOW_TITLE)
                if main_win.exists(timeout=2):
                    main_win.close()
            except Exception:
                pass
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=10)
