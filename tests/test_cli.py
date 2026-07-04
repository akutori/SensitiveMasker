import io
import json
import sys

import pytest

from cli.main import main
from tests.fixtures.synthetic_logs import FAKE_PHONE_1


@pytest.fixture
def profile_path(tmp_path):
    """Self-contained test profile (phone-number rule matching FAKE_PHONE_1).

    CLI tests build their own profile file rather than depending on any
    shipped rules/*.json, since the CLI must work with any profile the
    user points it at.
    """
    data = {
        "profile_name": "test",
        "rules": [
            {
                "name": "phone",
                "pattern_type": "regex",
                "pattern": r"0\d{1,4}-\d{1,4}-\d{3,4}",
                "mode": "random",
                "prefix": "__MASK_PHONE_",
            }
        ],
    }
    path = tmp_path / "test_profile.json"
    path.write_text(json.dumps(data), encoding="utf-8")
    return str(path)


def _run(monkeypatch, capsys, argv, stdin_text=None):
    if stdin_text is not None:
        monkeypatch.setattr(sys, "stdin", io.StringIO(stdin_text))
    exit_code = main(argv)
    captured = capsys.readouterr()
    return exit_code, captured.out, captured.err


def test_cli_stdin_stdout_masks_text(monkeypatch, capsys, profile_path):
    exit_code, out, _ = _run(
        monkeypatch,
        capsys,
        ["--profile", profile_path],
        stdin_text=f"caller={FAKE_PHONE_1}\n",
    )
    assert exit_code == 0
    assert "__MASK_PHONE_1__" in out
    assert FAKE_PHONE_1 not in out


def test_cli_file_input_output(tmp_path, profile_path):
    input_path = tmp_path / "in.log"
    output_path = tmp_path / "out.log"
    input_path.write_text(f"caller={FAKE_PHONE_1}\n", encoding="utf-8")

    exit_code = main(
        [
            "--profile", profile_path,
            "--input", str(input_path),
            "--output", str(output_path),
        ]
    )

    assert exit_code == 0
    masked = output_path.read_text(encoding="utf-8")
    assert "__MASK_PHONE_1__" in masked
    assert FAKE_PHONE_1 not in masked


def test_cli_missing_profile_arg_exits_with_usage_error(capsys):
    with pytest.raises(SystemExit) as exc_info:
        main([])
    assert exc_info.value.code == 2
    err = capsys.readouterr().err
    assert "required" in err.lower()


def test_cli_invalid_profile_path_exits_nonzero_with_clean_message(capsys):
    exit_code = main(["--profile", "does_not_exist.json"])
    assert exit_code == 1
    err = capsys.readouterr().err
    assert "Traceback" not in err


def test_cli_batch_mode_shared_mapping_across_files(tmp_path, profile_path):
    file_a = tmp_path / "a.log"
    file_b = tmp_path / "b.log"
    file_a.write_text(f"caller={FAKE_PHONE_1}\n", encoding="utf-8")
    file_b.write_text(f"caller={FAKE_PHONE_1}\n", encoding="utf-8")
    output_dir = tmp_path / "out"

    exit_code = main(
        [
            "--profile", profile_path,
            "--batch", str(file_a), str(file_b),
            "--output-dir", str(output_dir),
        ]
    )

    assert exit_code == 0
    out_a = (output_dir / "a.masked.log").read_text(encoding="utf-8")
    out_b = (output_dir / "b.masked.log").read_text(encoding="utf-8")
    assert "__MASK_PHONE_1__" in out_a
    # Shared MappingStore: same original value across files reuses the
    # same dummy (counter does not advance to _2__ for the second file).
    assert "__MASK_PHONE_1__" in out_b


def test_cli_batch_mode_reset_mapping_per_file(tmp_path, profile_path):
    file_a = tmp_path / "a.log"
    file_b = tmp_path / "b.log"
    file_a.write_text(f"caller={FAKE_PHONE_1}\n", encoding="utf-8")
    file_b.write_text(f"caller={FAKE_PHONE_1}\n", encoding="utf-8")
    output_dir = tmp_path / "out"

    exit_code = main(
        [
            "--profile", profile_path,
            "--batch", str(file_a), str(file_b),
            "--output-dir", str(output_dir),
            "--reset-mapping-per-file",
        ]
    )

    assert exit_code == 0
    out_a = (output_dir / "a.masked.log").read_text(encoding="utf-8")
    out_b = (output_dir / "b.masked.log").read_text(encoding="utf-8")
    # Reset per file: counter restarts at _1__ in both files independently.
    assert "__MASK_PHONE_1__" in out_a
    assert "__MASK_PHONE_1__" in out_b


def test_cli_batch_requires_output_dir(tmp_path, profile_path):
    file_a = tmp_path / "a.log"
    file_a.write_text("nothing\n", encoding="utf-8")
    with pytest.raises(SystemExit) as exc_info:
        main(["--profile", profile_path, "--batch", str(file_a)])
    assert exc_info.value.code == 2


def test_cli_batch_and_input_mutually_exclusive(tmp_path, profile_path):
    file_a = tmp_path / "a.log"
    file_a.write_text("nothing\n", encoding="utf-8")
    with pytest.raises(SystemExit) as exc_info:
        main(
            [
                "--profile", profile_path,
                "--input", str(file_a),
                "--batch", str(file_a),
                "--output-dir", str(tmp_path / "out"),
            ]
        )
    assert exc_info.value.code == 2
