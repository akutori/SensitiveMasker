from __future__ import annotations

import argparse
import sys
from pathlib import Path

from masking_core.masker import MappingStore, apply_profile
from masking_core.profile_io import ProfileLoadError, load_profile


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="cli.main",
        description="Mask sensitive info in text using a rule profile.",
    )
    parser.add_argument("--profile", required=True, help="Path to a RuleProfile JSON file")
    parser.add_argument("--encoding", default="utf-8", help="Text encoding for reading/writing files")
    parser.add_argument(
        "--reset-mapping-per-file",
        action="store_true",
        help="Batch mode only: reset MappingStore for each file instead of sharing one across the batch",
    )

    mode_group = parser.add_mutually_exclusive_group()
    mode_group.add_argument("--input", help="Input file path (default: stdin)")
    mode_group.add_argument(
        "--batch", nargs="+", metavar="INPUT", help="Batch-process multiple input files"
    )

    parser.add_argument("--output", help="Output file path (default: stdout); ignored if --batch is used")
    parser.add_argument("--output-dir", help="Output directory for --batch mode (required with --batch)")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.batch:
        if not args.output_dir:
            parser.error("--output-dir is required when using --batch")
        if args.output:
            parser.error("--output cannot be used together with --batch")
    elif args.output_dir:
        parser.error("--output-dir requires --batch")

    try:
        profile = load_profile(args.profile)
    except ProfileLoadError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    if args.batch:
        return _run_batch(args, profile)
    return _run_single(args, profile)


def _run_single(args: argparse.Namespace, profile) -> int:
    store = MappingStore()

    if args.input:
        text = Path(args.input).read_text(encoding=args.encoding)
    else:
        text = sys.stdin.read()

    masked, _ = apply_profile(text, profile, store)

    if args.output:
        Path(args.output).write_text(masked, encoding=args.encoding)
    else:
        sys.stdout.write(masked)

    return 0


def _run_batch(args: argparse.Namespace, profile) -> int:
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    store = MappingStore()
    for input_path_str in args.batch:
        if args.reset_mapping_per_file:
            store = MappingStore()

        input_path = Path(input_path_str)
        text = input_path.read_text(encoding=args.encoding)
        masked, store = apply_profile(text, profile, store)

        output_path = output_dir / f"{input_path.stem}.masked{input_path.suffix}"
        output_path.write_text(masked, encoding=args.encoding)

    return 0


if __name__ == "__main__":
    sys.exit(main())
