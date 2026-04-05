#!/usr/bin/env python3
"""
Download a supported local ASR model into the cache dir.

FunASR models are fetched from ModelScope and Qwen/Qwen3-ASR-1.7B is fetched
from Hugging Face. The local cache keeps the runtime offline-friendly after the
first download.
"""

from __future__ import annotations

import argparse
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


DEFAULT_BACKEND = "funasr"
DEFAULT_MODEL_ID_BY_BACKEND = {
    "funasr": "FunAudioLLM/Fun-ASR-Nano-2512",
    "qwen": "Qwen/Qwen3-ASR-1.7B",
}
DEFAULT_MODEL_URL_BY_BACKEND = {
    "funasr": "https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512",
    "qwen": "https://huggingface.co/Qwen/Qwen3-ASR-1.7B",
}
DEFAULT_LOCAL_DIR_BY_BACKEND = {
    "funasr": Path("./models/FunAudioLLM/Fun-ASR-Nano-2512"),
    "qwen": Path("./models/Qwen/Qwen3-ASR-1.7B"),
}
MODEL_CODE_NAME = "model.py"
REMOTE_CODE_BASE = "https://raw.githubusercontent.com/FunAudioLLM/Fun-ASR/main"
REMOTE_CODE_FILES = [
    "model.py",
    "ctc.py",
    "tools/__init__.py",
    "tools/utils.py",
]

ENV_MODEL_NAME = os.environ.get("VOICEINPUT_ASR_MODEL", "").strip().lower()
ENV_MODEL_ID = os.environ.get("VOICEINPUT_ASR_MODEL_ID", "").strip()
ENV_BACKEND = os.environ.get("VOICEINPUT_ASR_BACKEND", "").strip().lower()
ENV_SOURCE_URL = os.environ.get("VOICEINPUT_ASR_SOURCE_URL", "").strip()
ENV_LOCAL_DIR = os.environ.get("VOICEINPUT_ASR_MODEL_DIR", "").strip()


def normalize_backend_name(value: str | None) -> str | None:
    if not value:
        return None
    normalized = value.strip().lower()
    if normalized in {"funasr", "fun"}:
        return "funasr"
    if normalized in {"qwen", "qwen3", "qwen-asr"}:
        return "qwen"
    return None


def infer_backend_from_env() -> str:
    if ENV_MODEL_ID:
        return "qwen" if "qwen/qwen3-asr" in ENV_MODEL_ID.lower() else "funasr"

    backend = normalize_backend_name(ENV_MODEL_NAME)
    if backend:
        return backend

    backend = normalize_backend_name(ENV_BACKEND)
    if backend:
        return backend

    return DEFAULT_BACKEND


def has_required_model_files(local_dir: Path, backend: str) -> bool:
    if backend == "qwen":
        return local_dir.exists() and any(local_dir.iterdir())

    required_files = [
        local_dir / "config.yaml",
        local_dir / "model.pt",
        local_dir / MODEL_CODE_NAME,
        local_dir / "ctc.py",
        local_dir / "tools" / "__init__.py",
        local_dir / "tools" / "utils.py",
    ]
    return all(path.exists() for path in required_files)


def download_remote_code_files(local_dir: Path) -> None:
    for rel_path in REMOTE_CODE_FILES:
        target_path = local_dir / rel_path
        if target_path.exists():
            continue
        target_path.parent.mkdir(parents=True, exist_ok=True)
        tmp_fd, tmp_name = tempfile.mkstemp(dir=str(target_path.parent))
        os.close(tmp_fd)
        tmp_path = Path(tmp_name)
        url = f"{REMOTE_CODE_BASE}/{rel_path}"
        print(f"正在下载远程代码文件：{rel_path}")
        last_error = None
        downloaders = [
            [
                "curl",
                "-fsSL",
                "--http1.1",
                "--retry",
                "5",
                "--retry-delay",
                "1",
                "--connect-timeout",
                "20",
                "--max-time",
                "60",
                "-o",
                str(tmp_path),
                url,
            ],
            [
                "wget",
                "--quiet",
                "--tries=5",
                "--waitretry=1",
                "--timeout=20",
                "-O",
                str(tmp_path),
                url,
            ],
        ]
        try:
            for command in downloaders:
                try:
                    if tmp_path.exists():
                        tmp_path.unlink()
                    result = subprocess.run(command, stderr=subprocess.PIPE, check=False)
                    if result.returncode == 0 and tmp_path.exists():
                        tmp_path.replace(target_path)
                        last_error = None
                        break
                    last_error = subprocess.CalledProcessError(
                        result.returncode, command, stderr=result.stderr
                    )
                    print(f"下载失败，正在切换下载器：{last_error}")
                except OSError as exc:
                    last_error = exc
                    print(f"下载失败，正在切换下载器：{exc}")
            if last_error is not None:
                raise RuntimeError(f"下载远程代码文件失败：{rel_path}: {last_error}")
        finally:
            if tmp_path.exists():
                try:
                    tmp_path.unlink()
                except OSError:
                    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="部署本地 ASR 模型")
    parser.add_argument(
        "--backend",
        choices=sorted(DEFAULT_MODEL_ID_BY_BACKEND.keys()),
        default=None,
        help="选择 ASR 模型后端",
    )
    parser.add_argument(
        "--model",
        choices=sorted(DEFAULT_MODEL_ID_BY_BACKEND.keys()),
        default=None,
        help="选择 ASR 模型（backend 的别名）",
    )
    parser.add_argument("--model-id", default=None, help="模型 ID")
    parser.add_argument("--source-url", default=None, help="模型来源页面 URL")
    parser.add_argument(
        "--local-dir",
        default=None,
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
    if args.model and args.backend and args.model != args.backend:
        print(
            f"--backend={args.backend} 与 --model={args.model} 冲突，请保留一个",
            file=sys.stderr,
        )
        return 2

    cli_backend = args.model or args.backend
    if cli_backend:
        backend = cli_backend
    else:
        backend = infer_backend_from_env()

    if args.model_id:
        model_id = args.model_id
    elif cli_backend is None and ENV_MODEL_ID:
        model_id = ENV_MODEL_ID
    else:
        model_id = DEFAULT_MODEL_ID_BY_BACKEND[backend]

    if args.source_url:
        source_url = args.source_url
    elif cli_backend is None and ENV_SOURCE_URL:
        source_url = ENV_SOURCE_URL
    else:
        source_url = DEFAULT_MODEL_URL_BY_BACKEND[backend]

    if args.local_dir:
        local_dir = Path(args.local_dir)
    elif cli_backend is None and ENV_LOCAL_DIR:
        local_dir = Path(ENV_LOCAL_DIR)
    else:
        local_dir = Path(DEFAULT_LOCAL_DIR_BY_BACKEND[backend])

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
            "默认不会自动安装 CUDA。若传入 --install-cuda 且检测到 NVIDIA GPU，脚本会尝试安装 CUDA 版 PyTorch wheels；否则请先安装 CUDA 版 PyTorch 以及匹配的 NVIDIA 驱动 / CUDA runtime，再进行推理。"
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

    if args.skip_existing and local_dir.exists():
        if has_required_model_files(local_dir, backend):
            print(f"模型已存在：{local_dir}")
            return 0
        if backend == "funasr":
            print("发现模型目录缺少远程代码文件，正在补齐...")
            download_remote_code_files(local_dir)
            if has_required_model_files(local_dir, backend):
                print(f"模型已存在：{local_dir}")
                return 0

    if backend == "funasr":
        try:
            from modelscope import snapshot_download
        except ImportError:
            print(
                "需要先安装 modelscope。可执行：uv pip install -r scripts/requirements-asr-base.txt",
                file=sys.stderr,
            )
            return 1
    else:
        try:
            from huggingface_hub import snapshot_download
        except ImportError:
            print(
                "需要先安装 huggingface_hub。可执行：uv pip install -r scripts/requirements-asr-base.txt",
                file=sys.stderr,
            )
            return 1

    print(f"正在下载模型：{model_id}")
    print(f"来源：{source_url}")
    print(f"后端：{backend}")
    print(f"目标目录：{local_dir}")
    print(f"部署提示设备：{target_device}")

    if backend == "funasr":
        downloaded = snapshot_download(
            model_id,
            revision=args.revision,
            local_dir=str(local_dir),
        )
        download_remote_code_files(local_dir)
    else:
        downloaded = snapshot_download(
            repo_id=model_id,
            revision=args.revision,
            local_dir=str(local_dir),
            local_dir_use_symlinks=False,
        )

    print(f"下载完成：{downloaded}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
