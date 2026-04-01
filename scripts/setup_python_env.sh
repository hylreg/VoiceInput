#!/usr/bin/env bash
set -euo pipefail

if ! command -v uv >/dev/null 2>&1; then
  echo "需要先安装 uv。安装说明：https://docs.astral.sh/uv/" >&2
  exit 1
fi

uv venv .venv
uv pip install -r scripts/requirements-asr.txt

echo "Python 环境已准备好，位于 .venv"
echo "使用方式：source .venv/bin/activate"
