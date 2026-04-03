#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

refresh_cargo_path() {
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if [[ -f "${HOME}/.cargo/env" ]]; then
    # shellcheck disable=SC1090
    source "${HOME}/.cargo/env"
  fi

  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if command -v rustup >/dev/null 2>&1; then
    local cargo_path
    cargo_path="$(rustup which cargo 2>/dev/null || true)"
    if [[ -n "$cargo_path" ]]; then
      export PATH="$(cd "$(dirname "$cargo_path")" && pwd):$PATH"
    fi
  fi
}

ensure_uv() {
  if command -v uv >/dev/null 2>&1; then
    return 0
  fi
  echo "需要先安装 uv。安装说明：https://docs.astral.sh/uv/" >&2
  exit 1
}

run_prepare=false
restart_server=false
stop_server=false
app_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prepare)
      run_prepare=true
      shift
      ;;
    --restart-server)
      restart_server=true
      shift
      ;;
    --stop-server)
      stop_server=true
      shift
      ;;
    --help|-h)
      cat >&2 <<'EOF'
用法：
  scripts/dev_linux_streaming.sh [--prepare] [--restart-server] [--stop-server] [-- 传给 Linux 常驻应用的参数...]

说明：
  - 会启动一个常驻 FunASR 开发服务，模型只加载一次
  - Rust Linux 常驻应用会连接这个服务，重启前端时无需重新加载模型
  - 默认会复用已存在的开发服务，不会每次都重新加载模型
  - 如果你想强制重新加载模型，传 `--restart-server`
  - 如果你想只关闭开发服务，传 `--stop-server`
  - 默认会直接启动 Linux 常驻应用，并强制使用 `--double-ctrl-window-ms 300`
  - 后面的参数会原样传给它；如果你自己传了 `--double-ctrl-window-ms`，会覆盖默认值
  - 如果首次运行还没有准备好环境，先加 --prepare

示例：
  scripts/dev_linux_streaming.sh
  scripts/dev_linux_streaming.sh --prepare
  scripts/dev_linux_streaming.sh --restart-server
  scripts/dev_linux_streaming.sh --stop-server
  scripts/dev_linux_streaming.sh -- --silence-stop-ms 700
EOF
      exit 0
      ;;
    --)
      shift
      app_args+=("$@")
      break
      ;;
    *)
      app_args+=("$1")
      shift
      ;;
  esac
done

has_app_arg() {
  local needle="$1"
  local arg
  for arg in "${app_args[@]}"; do
    if [[ "$arg" == "$needle" ]]; then
      return 0
    fi
  done
  return 1
}

if ! has_app_arg "--double-ctrl-window-ms"; then
  app_args=("--double-ctrl-window-ms" "300" "${app_args[@]}")
fi

socket_path="${VOICEINPUT_FUNASR_SOCKET_PATH:-/tmp/voiceinput-funasr.sock}"
server_pid_file="${VOICEINPUT_FUNASR_PID_FILE:-/tmp/voiceinput-funasr.pid}"
server_log="${VOICEINPUT_FUNASR_LOG_FILE:-/tmp/voiceinput-funasr.log}"

socket_is_alive() {
  local target_socket="$1"
  python3 - "$target_socket" <<'PY'
import socket
import sys

path = sys.argv[1]
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.settimeout(0.2)
try:
    sock.connect(path)
except OSError:
    raise SystemExit(1)
finally:
    sock.close()
raise SystemExit(0)
PY
}

cd "$REPO_ROOT"
ensure_uv
refresh_cargo_path

if [[ "$run_prepare" == true ]]; then
  bash scripts/bootstrap.sh --skip-existing
fi

if [[ ! -f ".venv/bin/python" ]]; then
  echo "未找到 .venv。请先运行 scripts/bootstrap.sh 或 scripts/dev_linux_streaming.sh --prepare" >&2
  exit 2
fi

server_running=false
if [[ -S "$socket_path" ]] && socket_is_alive "$socket_path"; then
  server_running=true
elif [[ -S "$socket_path" ]]; then
  rm -f "$socket_path"
fi

if [[ "$stop_server" == true ]]; then
  if [[ -f "$server_pid_file" ]]; then
    server_pid="$(cat "$server_pid_file" 2>/dev/null || true)"
    if [[ -n "${server_pid:-}" ]] && kill -0 "$server_pid" >/dev/null 2>&1; then
      echo "正在停止 FunASR 开发服务：$server_pid"
      kill "$server_pid" >/dev/null 2>&1 || true
      for _ in $(seq 1 20); do
        if ! kill -0 "$server_pid" >/dev/null 2>&1; then
          break
        fi
        sleep 0.2
      done
    fi
  fi
  rm -f "$server_pid_file" "$socket_path"
  echo "FunASR 开发服务已停止"
  exit 0
fi

if [[ "$restart_server" == true ]]; then
  if [[ -f "$server_pid_file" ]]; then
    server_pid="$(cat "$server_pid_file" 2>/dev/null || true)"
    if [[ -n "${server_pid:-}" ]] && kill -0 "$server_pid" >/dev/null 2>&1; then
      echo "正在重启 FunASR 开发服务：$server_pid"
      kill "$server_pid" >/dev/null 2>&1 || true
      for _ in $(seq 1 20); do
        if ! kill -0 "$server_pid" >/dev/null 2>&1; then
          break
        fi
        sleep 0.2
      done
    fi
  fi
  rm -f "$server_pid_file" "$socket_path"
  server_running=false
fi

if [[ "$server_running" == false ]]; then
  echo "正在启动常驻 FunASR 开发服务"
  nohup uv run -- python scripts/funasr_stream_server.py \
    --socket-path "$socket_path" \
    --model-dir "./models/FunAudioLLM/Fun-ASR-Nano-2512" \
    >"$server_log" 2>&1 &
  server_pid=$!
  echo "$server_pid" >"$server_pid_file"

  for _ in $(seq 1 300); do
    if [[ -S "$socket_path" ]]; then
      break
    fi
    if ! kill -0 "$server_pid" >/dev/null 2>&1; then
      echo "FunASR 开发服务启动失败，日志如下：" >&2
      cat "$server_log" >&2 || true
      rm -f "$server_pid_file"
      exit 1
    fi
    sleep 1
  done

  if [[ ! -S "$socket_path" ]]; then
    echo "等待 FunASR 开发服务就绪超时，日志如下：" >&2
    cat "$server_log" >&2 || true
    rm -f "$server_pid_file"
    exit 1
  fi
else
  echo "复用已有 FunASR 开发服务：$socket_path"
fi

echo "FunASR 开发服务已就绪：$socket_path"
VOICEINPUT_FUNASR_SOCKET="$socket_path" \
  uv run -- cargo run -p voice-input-linux --features ibus --bin voice-input-linux-app -- "${app_args[@]}"
