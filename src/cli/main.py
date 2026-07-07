from __future__ import annotations

import argparse
import sys
from pathlib import Path

from masking_core.masker import MappingStore, apply_profile
from masking_core.profile_io import ProfileLoadError, load_profile


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="cli.main",
        description="ルールプロファイルを使ってテキスト中の機微情報をマスキングします。",
    )
    parser.add_argument("--profile", required=True, help="RuleProfile JSONファイルのパス")
    parser.add_argument("--encoding", default="utf-8", help="ファイル読み書き時の文字エンコーディング")
    parser.add_argument(
        "--reset-mapping-per-file",
        action="store_true",
        help="バッチモード専用: MappingStoreをファイル間で共有せず、ファイルごとにリセットする",
    )

    mode_group = parser.add_mutually_exclusive_group()
    mode_group.add_argument("--input", help="入力ファイルパス(省略時は標準入力)")
    mode_group.add_argument(
        "--batch", nargs="+", metavar="INPUT", help="複数の入力ファイルをバッチ処理する"
    )

    parser.add_argument("--output", help="出力ファイルパス(省略時は標準出力。--batch使用時は無視される)")
    parser.add_argument("--output-dir", help="--batch使用時の出力先ディレクトリ(--batch使用時は必須)")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.batch:
        if not args.output_dir:
            parser.error("--batch使用時は --output-dir が必須です")
        if args.output:
            parser.error("--output は --batch と併用できません")
    elif args.output_dir:
        parser.error("--output-dir は --batch と併用してください")

    try:
        profile = load_profile(args.profile)
    except ProfileLoadError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    if args.batch:
        return _run_batch(args, profile)
    return _run_single(args, profile)


def _reconfigure_encoding(stream, encoding: str) -> None:
    # Real stdin/stdout are TextIOWrapper and default to the console's
    # codepage, which may not match the bytes actually being piped in
    # (e.g. PowerShell sending UTF-8). Test doubles like io.StringIO have
    # no encoding concept at all, so skip them via the hasattr guard.
    if hasattr(stream, "reconfigure"):
        stream.reconfigure(encoding=encoding)


def _run_single(args: argparse.Namespace, profile) -> int:
    store = MappingStore()

    if args.input:
        text = Path(args.input).read_text(encoding=args.encoding)
    else:
        _reconfigure_encoding(sys.stdin, args.encoding)
        text = sys.stdin.read()

    masked, _ = apply_profile(text, profile, store)

    if args.output:
        Path(args.output).write_text(masked, encoding=args.encoding)
    else:
        _reconfigure_encoding(sys.stdout, args.encoding)
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
