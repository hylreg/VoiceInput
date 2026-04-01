#!/usr/bin/env python3
"""
Download FunAudioLLM/Fun-ASR-Nano-2512 from ModelScope into the local cache dir.

This keeps the runtime offline-friendly after the first download.
"""

from __future__ import annotations

import argparse
import platform
import shutil
import subprocess
import sys
from pathlib import Path


DEFAULT_MODEL_ID = "FunAudioLLM/Fun-ASR-Nano-2512"
DEFAULT_MODEL_URL = "https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512"
DEFAULT_LOCAL_DIR = Path("./models/FunAudioLLM/Fun-ASR-Nano-2512")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="部署本地 Fun-ASR 模型")
    parser.add_argument("--model-id", default=DEFAULT_MODEL_ID, help="ModelScope 模型 ID")
    parser.add_argument(
        "--local-dir",
        default=str(DEFAULT_LOCAL_DIR),
        help="模型存放目录",
    )
    parser.add_argument(
        "--revision",
        default="master",
        help="要下载的模型版本",
    )
    parser.add_argument(
        "--skip-existing",
        action="store_true",
        help="目标目录已存在时不重新下载",
    )
    parser.add_argument(
        "--device",
        choices=["auto", "cpu", "cuda", "mps"],
        default="auto",
        help="写入部署提示时使用的运行设备",
    )
    parser.add_argument(
        "--install-cuda",
        action="store_true",
        help="检测到支持 CUDA 的 NVIDIA GPU 时安装 CUDA 版 PyTorch wheels",
    )
    parser.add_argument(
        "--cuda-wheel-index",
        default="https://download.pytorch.org/whl/cu124",
        help="CUDA 安装时使用的 PyTorch wheel 索引地址",
    )
    return parser.parse_args()


def detect_nvidia_gpu() -> bool:
    if platform.system() not in {"Linux", "Windows"}:
        return False

    if shutil.which("nvidia-smi"):
        return True

    return False


def detect_mps() -> bool:
    if platform.system() != "Darwin":
        return False

    try:
        import torch
    except ImportError:
        return False

    return bool(getattr(torch.backends, "mps", None) and torch.backends.mps.is_available())


def main() -> int:
    args = parse_args()
    local_dir = Path(args.local_dir)
    has_nvidia = detect_nvidia_gpu()
    has_mps = detect_mps()

    if args.device == "auto":
        if has_mps:
            target_device = "mps"
        elif has_nvidia:
            target_device = "cuda"
        else:
            target_device = "cpu"
    else:
        target_device = args.device

    print(f"检测到 NVIDIA GPU：{'是' if has_nvidia else '否'}")
    print(f"检测到 MPS：{'是' if has_mps else '否'}")
    print(f"选择的运行设备：{target_device}")

    if target_device == "cuda":
        print(
            "本脚本不会自动安装 CUDA。请先安装 CUDA 版 PyTorch 以及匹配的 NVIDIA 驱动 / CUDA runtime，再进行推理。"
        )
    elif target_device == "mps":
        print(
            "MPS 是 macOS / Apple Silicon 上的 PyTorch 后端。请确认已安装支持 MPS 的 PyTorch 版本。"
        )

    if args.install_cuda:
        if platform.system() == "Darwin":
            print(
                "macOS 不支持 CUDA。请改用 MPS 后端，并安装支持 MPS 的 macOS 版 PyTorch。",
                file=sys.stderr,
            )
            return 1

        if not has_nvidia:
            print(
                "未检测到 NVIDIA GPU，已跳过 CUDA 安装。"
                "如果是支持的 Mac，请使用 --device mps；否则可用 --device cpu。",
                file=sys.stderr,
            )
            return 1

        print("正在安装 CUDA 版 PyTorch wheels")
        print(f"wheel 索引：{args.cuda_wheel_index}")
        cmd = [
            sys.executable,
            "-m",
            "pip",
            "install",
            "--upgrade",
            "torch",
            "torchvision",
            "torchaudio",
            "--index-url",
            args.cuda_wheel_index,
        ]
        result = subprocess.run(cmd, check=False)
        if result.returncode != 0:
            print("CUDA 版 PyTorch 安装失败", file=sys.stderr)
            return result.returncode

    if args.skip_existing and local_dir.exists() and any(local_dir.iterdir()):
        print(f"模型已存在：{local_dir}")
        return 0

    try:
        from modelscope import snapshot_download
    except ImportError as exc:
        print(
            "需要先安装 modelscope。可执行：uv pip install -r scripts/requirements-asr.txt",
            file=sys.stderr,
        )
        return 1

    print(f"正在下载模型：{args.model_id}")
    print(f"来源：{DEFAULT_MODEL_URL}")
    print(f"目标目录：{local_dir}")
    print(f"部署提示设备：{target_device}")

    downloaded = snapshot_download(
        args.model_id,
        revision=args.revision,
        local_dir=str(local_dir),
    )

    print(f"下载完成：{downloaded}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
