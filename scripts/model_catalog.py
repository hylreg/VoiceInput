#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = REPO_ROOT / "config" / "models.json"


def load_catalog() -> dict:
    with CATALOG_PATH.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def normalize_choice(choice: str) -> str:
    catalog = load_catalog()
    aliases = catalog.get("aliases", {})
    normalized = aliases.get(choice.strip().lower())
    if not normalized:
        raise KeyError(f"unsupported model alias: {choice}")
    return normalized


def model_spec(choice: str) -> dict:
    catalog = load_catalog()
    normalized = normalize_choice(choice)
    spec = catalog.get("models", {}).get(normalized)
    if not spec:
        raise KeyError(f"missing model spec: {normalized}")
    return {"name": normalized, **spec}


def shell_quote(value: str) -> str:
    return "'" + value.replace("'", "'\"'\"'") + "'"


def render_env(choice: str) -> str:
    spec = model_spec(choice)
    lines = [
        f"export VOICEINPUT_ASR_MODEL={shell_quote(spec['name'])}",
        f"export VOICEINPUT_ASR_BACKEND={shell_quote(spec['backend'])}",
        f"export VOICEINPUT_ASR_MODEL_ID={shell_quote(spec['model_id'])}",
        f"export VOICEINPUT_ASR_SOURCE_URL={shell_quote(spec['source_url'])}",
        f"export VOICEINPUT_ASR_MODEL_DIR={shell_quote(spec['model_dir'])}",
        f"export VOICEINPUT_ASR_DEVICE='auto'",
        f"export VOICEINPUT_ASR_LANGUAGE='中文'",
        f"export VOICEINPUT_ASR_ITN='true'",
        f"export VOICEINPUT_ASR_HOTWORDS=''",
    ]
    remote_code = spec.get("remote_code", "")
    if remote_code:
        lines.insert(
            5,
            f"export VOICEINPUT_ASR_REMOTE_CODE={shell_quote(remote_code)}",
        )
    return "\n".join(lines)


def render_config_file(choice: str) -> str:
    normalized = normalize_choice(choice)
    return "\n".join(
        [
            "# VoiceInput shared configuration.",
            "# Generated from config/models.json via scripts/model_catalog.py.",
            "# To switch the default model, run:",
            "#   scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b>",
            "# Explicit command-line flags and environment variables still override these values.",
            f"# Active model: {normalized}",
            render_env(normalized),
            "",
        ]
    )


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(
            "usage: model_catalog.py <normalize|get|render-env|render-config-file> ...",
            file=sys.stderr,
        )
        return 2

    command = argv[1]
    try:
        if command == "normalize":
            if len(argv) != 3:
                return 2
            print(normalize_choice(argv[2]))
            return 0

        if command == "get":
            if len(argv) != 4:
                return 2
            spec = model_spec(argv[2])
            value = spec.get(argv[3], "")
            if isinstance(value, str):
                print(value)
                return 0
            print(json.dumps(value, ensure_ascii=False))
            return 0

        if command == "render-env":
            if len(argv) != 3:
                return 2
            print(render_env(argv[2]))
            return 0

        if command == "render-config-file":
            if len(argv) != 3:
                return 2
            print(render_config_file(argv[2]), end="")
            return 0
    except KeyError as exc:
        print(str(exc), file=sys.stderr)
        return 2

    print(f"unknown command: {command}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
