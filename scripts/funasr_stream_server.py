#!/usr/bin/env python3
"""
Keep a FunASR model loaded in a long-lived Unix socket server.

This is meant for development/debugging so the Rust app can reconnect without
reloading the model every time it restarts.
"""

from __future__ import annotations

import argparse
import base64
import contextlib
import json
import os
import signal
import socketserver
import sys
import tempfile
import wave
from pathlib import Path

import numpy as np
import torch
from funasr import AutoModel


DEFAULT_MODEL_DIR = Path("./models/FunAudioLLM/Fun-ASR-Nano-2512")
DEFAULT_REMOTE_CODE = "./model.py"
DEFAULT_SOCKET_PATH = Path("/tmp/voiceinput-funasr.sock")


def resolve_remote_code(model_dir: Path, remote_code: str) -> str:
    if not remote_code:
        return remote_code

    candidate = Path(remote_code)
    if not candidate.is_absolute():
        local_candidate = model_dir / candidate
        if local_candidate.exists():
            candidate = local_candidate
    if candidate.is_dir():
        candidate = candidate / "model.py"
    model_py = model_dir / "model.py"
    if model_py.exists():
        candidate = model_py
    return str(candidate)


def pick_device(device: str) -> str:
    if device != "auto":
        return device

    if torch.cuda.is_available():
        return "cuda"
    if hasattr(torch.backends, "mps") and torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def load_model(model_dir: Path, remote_code: str, device: str) -> AutoModel:
    remote_code = resolve_remote_code(model_dir, remote_code)
    device = pick_device(device)

    with contextlib.redirect_stdout(sys.stderr):
        return AutoModel(
            model=str(model_dir),
            trust_remote_code=True,
            remote_code=remote_code,
            device=device,
            disable_update=True,
            log_level="ERROR",
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run a long-lived FunASR dev socket server")
    parser.add_argument("--model-dir", default=str(DEFAULT_MODEL_DIR))
    parser.add_argument("--remote-code", default=DEFAULT_REMOTE_CODE)
    parser.add_argument("--socket-path", default=str(DEFAULT_SOCKET_PATH))
    parser.add_argument(
        "--device",
        choices=["auto", "cpu", "cuda", "mps"],
        default="auto",
    )
    return parser.parse_args()


class FunAsrStreamServer(socketserver.UnixStreamServer):
    allow_reuse_address = True


class FunAsrStreamHandler(socketserver.StreamRequestHandler):
    model: AutoModel

    def handle(self) -> None:
        pending_samples = np.array([], dtype=np.float32)
        preview_window_seconds = 6
        sample_rate = 16_000
        self.wfile.write(json.dumps({"ready": True}).encode("utf-8") + b"\n")
        self.wfile.flush()

        for raw_line in self.rfile:
            line = raw_line.strip()
            if not line:
                continue

            try:
                request = json.loads(line)
                action = request.get("action", "chunk")

                if action == "reset":
                    pending_samples = np.array([], dtype=np.float32)
                    self._write_json({"ok": True})
                    continue

                if action != "chunk":
                    self._write_json({"error": f"unknown action: {action}"})
                    continue

                pcm = base64.b64decode(request["pcm_b64"])
                new_samples = np.frombuffer(pcm, dtype=np.int16).astype(np.float32) / 32768.0
                preview_window_seconds = int(request.get("preview_window_seconds", preview_window_seconds))
                sample_rate = int(request.get("sample_rate", sample_rate))
                is_final = request.get("is_final", False)
                pending_samples = np.concatenate([pending_samples, new_samples])
                if pending_samples.size == 0:
                    self._write_json({"text": "", "is_final": is_final})
                    continue

                if is_final:
                    inference_samples = pending_samples
                else:
                    preview_window_samples = max(int(preview_window_seconds * sample_rate), sample_rate)
                    inference_samples = pending_samples[-preview_window_samples:]

                text = self._transcribe_samples(
                    inference_samples,
                    sample_rate,
                    request.get("hotwords", []),
                    request.get("language"),
                    request.get("itn", True),
                )

                if is_final:
                    pending_samples = np.array([], dtype=np.float32)

                self._write_json({"text": text, "is_final": is_final})
            except Exception as exc:  # pragma: no cover - surfacing to caller is enough
                self._write_json({"error": str(exc)})

    def _transcribe_samples(self, samples: np.ndarray, sample_rate: int, hotwords, language, itn) -> str:
        if samples.size == 0:
            return ""

        int16_samples = np.clip(samples * 32768.0, -32768, 32767).astype(np.int16)
        with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
            wav_path = Path(tmp.name)

        try:
            with wave.open(str(wav_path), "wb") as wav_file:
                wav_file.setnchannels(1)
                wav_file.setsampwidth(2)
                wav_file.setframerate(sample_rate)
                wav_file.writeframes(int16_samples.tobytes())

            with contextlib.redirect_stdout(sys.stderr):
                res = self.model.generate(
                    input=[str(wav_path)],
                    cache={},
                    batch_size=1,
                    hotwords=hotwords,
                    language=language,
                    itn=itn,
                )

            if isinstance(res, list) and res and isinstance(res[0], dict):
                return str(res[0].get("text", "")).strip()
            return ""
        finally:
            if wav_path.exists():
                wav_path.unlink()

    def _write_json(self, payload: dict) -> None:
        self.wfile.write(json.dumps(payload, ensure_ascii=False).encode("utf-8") + b"\n")
        self.wfile.flush()


def main() -> int:
    args = parse_args()
    model_dir = Path(args.model_dir)
    socket_path = Path(args.socket_path)

    if socket_path.exists():
        socket_path.unlink()

    print(f"正在预加载 FunASR 开发调试服务：{model_dir}", file=sys.stderr)
    model = load_model(model_dir, args.remote_code, args.device)
    FunAsrStreamHandler.model = model

    server = FunAsrStreamServer(str(socket_path), FunAsrStreamHandler)
    print(json.dumps({"ready": True, "socket_path": str(socket_path)}), flush=True)

    def shutdown_handler(_signum, _frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGINT, shutdown_handler)
    signal.signal(signal.SIGTERM, shutdown_handler)

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        if socket_path.exists():
            socket_path.unlink()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
