#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# shellcheck disable=SC1090
source "$SCRIPT_DIR/voiceinput_config.sh"
voiceinput_load_config

voiceinput_configure_rust_mirror() {
  export RUSTUP_DIST_SERVER="${RUSTUP_DIST_SERVER:-https://mirrors.aliyun.com/rustup}"
  export RUSTUP_UPDATE_ROOT="${RUSTUP_UPDATE_ROOT:-https://mirrors.aliyun.com/rustup/rustup}"
}

voiceinput_run_without_proxy() {
  env \
    -u http_proxy -u https_proxy -u all_proxy \
    -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
    "$@"
}

voiceinput_clear_proxy_env() {
  unset http_proxy https_proxy all_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY
}

voiceinput_refresh_cargo_path() {
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

voiceinput_ensure_cargo() {
  voiceinput_refresh_cargo_path
  if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
    return 0
  fi

  if command -v rustup >/dev/null 2>&1; then
    echo "检测到 rustup，但 cargo/rustc 未就绪，正在尝试修复环境..." >&2
    voiceinput_refresh_cargo_path
  else
    echo "未找到 Rust 工具链，正在使用阿里云源自动安装 rustup..." >&2
    voiceinput_install_rustup
  fi

  if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    echo "未能自动准备好 cargo/rustc。请手动检查 rustup 安装是否成功。" >&2
    exit 1
  fi
}

voiceinput_install_rustup() {
  voiceinput_configure_rust_mirror

  if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1 && ! command -v python3 >/dev/null 2>&1; then
    echo "未找到 curl、wget 或 python3，无法自动安装 Rust 工具链。" >&2
    exit 1
  fi

  local rustup_init="${TMPDIR:-/tmp}/rustup-init.sh"
  if command -v curl >/dev/null 2>&1; then
    voiceinput_run_without_proxy curl -fsSL --retry 3 --noproxy '*' https://sh.rustup.rs -o "$rustup_init"
  else
    if command -v wget >/dev/null 2>&1; then
      voiceinput_run_without_proxy wget -qO "$rustup_init" --no-proxy https://sh.rustup.rs
    else
      voiceinput_run_without_proxy python3 - "$rustup_init" https://sh.rustup.rs <<'PY'
import sys
import urllib.request

out_path = sys.argv[1]
url = sys.argv[2]
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
with opener.open(url) as response, open(out_path, "wb") as output:
    output.write(response.read())
PY
    fi
  fi

  chmod +x "$rustup_init"
  voiceinput_run_without_proxy sh "$rustup_init" -y --default-toolchain stable --profile minimal

  voiceinput_refresh_cargo_path
}

voiceinput_ensure_rustfmt() {
  if ! command -v rustup >/dev/null 2>&1; then
    return 0
  fi

  if rustup component list --installed | grep -q '^rustfmt '; then
    return 0
  fi

  echo "正在安装 rustfmt 组件"
  voiceinput_run_without_proxy rustup component add rustfmt
}

voiceinput_ensure_uv() {
  if command -v uv >/dev/null 2>&1; then
    return 0
  fi

  echo "需要先安装 uv。安装说明：https://docs.astral.sh/uv/" >&2
  exit 1
}

voiceinput_find_cargo_bin() {
  local cargo_bin
  cargo_bin="$(command -v cargo || true)"
  if [[ -z "$cargo_bin" && -x "${HOME}/.cargo/bin/cargo" ]]; then
    cargo_bin="${HOME}/.cargo/bin/cargo"
  fi
  printf '%s\n' "$cargo_bin"
}

voiceinput_run_cli_linux() {
  local cargo_bin
  cargo_bin="$(voiceinput_find_cargo_bin)"
  if [[ -z "$cargo_bin" ]]; then
    echo "未找到 cargo，可先执行 scripts/voiceinput.sh bootstrap" >&2
    exit 1
  fi

  uv run -- "$cargo_bin" run -p voice-input-linux --features ibus -- "$@"
}

voiceinput_run_bootstrap_args() {
  if (($# > 0)); then
    voiceinput_bootstrap_impl "$@"
  else
    voiceinput_bootstrap_impl
  fi
}

voiceinput_run_platform_smoke() {
  local platform="$1"
  local audio_file="$2"
  local backend="${3:-ibus}"

  echo "正在运行 Linux smoke"
  voiceinput_linux_smoke_impl --audio-file "$audio_file" --backend "$backend"
}

voiceinput_run_platform_live() {
  local platform="$1"
  local backend="${2:-ibus}"

  voiceinput_ensure_cargo
  voiceinput_ensure_uv

  echo "正在启动 Linux 常驻托盘版"
  voiceinput_refresh_cargo_path
  voiceinput_run_cli_linux live --backend "$backend"
}

voiceinput_ensure_linux_dev_deps() {
  local -a required_packages=(
    pkg-config
    libx11-dev
    libasound2-dev
    portaudio19-dev
  )
  local -a missing_packages=()

  for package in "${required_packages[@]}"; do
    if ! dpkg -s "$package" >/dev/null 2>&1; then
      missing_packages+=("$package")
    fi
  done

  if ((${#missing_packages[@]} == 0)); then
    return 0
  fi

  if ! command -v apt-get >/dev/null 2>&1; then
    echo "缺少 Linux 依赖：${missing_packages[*]}" >&2
    echo "当前系统没有 apt-get，无法自动安装这些包。" >&2
    exit 2
  fi

  local apt_cmd
  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    apt_cmd=(apt-get)
  elif command -v sudo >/dev/null 2>&1; then
    apt_cmd=(sudo apt-get)
  else
    echo "缺少 Linux 依赖：${missing_packages[*]}" >&2
    echo "需要 root 权限或 sudo 才能自动安装这些包。" >&2
    exit 2
  fi

  echo "正在自动安装 Linux 依赖：${missing_packages[*]}"
  "${apt_cmd[@]}" update
  DEBIAN_FRONTEND=noninteractive "${apt_cmd[@]}" install -y "${missing_packages[@]}"
}

voiceinput_normalize_model_choice() {
  python3 "$SCRIPT_DIR/model_catalog.py" normalize "${1:-}"
}

voiceinput_model_field_for_choice() {
  local choice="$1"
  local field="$2"
  python3 "$SCRIPT_DIR/model_catalog.py" get "$choice" "$field"
}

voiceinput_model_backend_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "backend"
}

voiceinput_model_id_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "model_id"
}

voiceinput_model_source_url_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "source_url"
}

voiceinput_model_local_dir_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "model_dir"
}

voiceinput_model_remote_code_for_choice() {
  voiceinput_model_field_for_choice "${1:-}" "remote_code"
}

voiceinput_apply_model_choice_env() {
  local choice
  choice="$(voiceinput_normalize_model_choice "${1:-}")" || return 1

  export VOICEINPUT_ASR_MODEL="$choice"
  export VOICEINPUT_ASR_BACKEND="$(voiceinput_model_backend_for_choice "$choice")"
  export VOICEINPUT_ASR_MODEL_ID="$(voiceinput_model_id_for_choice "$choice")"
  export VOICEINPUT_ASR_SOURCE_URL="$(voiceinput_model_source_url_for_choice "$choice")"
  export VOICEINPUT_ASR_MODEL_DIR="$(voiceinput_model_local_dir_for_choice "$choice")"
  local remote_code
  remote_code="$(voiceinput_model_remote_code_for_choice "$choice")"
  if [[ -n "$remote_code" ]]; then
    export VOICEINPUT_ASR_REMOTE_CODE="$remote_code"
  else
    unset VOICEINPUT_ASR_REMOTE_CODE
  fi
}

voiceinput_config_file_path() {
  printf '%s\n' "${VOICEINPUT_CONFIG_FILE:-$REPO_ROOT/config/voiceinput.env}"
}

voiceinput_write_model_config() {
  local model="$1"
  local config_file="${2:-$(voiceinput_config_file_path)}"
  local normalized_model
  normalized_model="$(voiceinput_normalize_model_choice "$model")" || return 1

  local tmp_file
  tmp_file="$(mktemp "${config_file}.XXXXXX")"

  python3 "$SCRIPT_DIR/model_catalog.py" render-config-file "$normalized_model" >"$tmp_file"

  mv "$tmp_file" "$config_file"
}

VOICEINPUT_EXPANDED_MODEL_ARGS=()

voiceinput_expand_model_args() {
  local mode="$1"
  local model="$2"
  local normalized_model
  if ! normalized_model="$(voiceinput_normalize_model_choice "$model")"; then
    return 1
  fi

  VOICEINPUT_EXPANDED_MODEL_ARGS=()
  case "$normalized_model" in
    qwen-0.6b)
      VOICEINPUT_EXPANDED_MODEL_ARGS=(
        "--backend" "$(voiceinput_model_backend_for_choice "$normalized_model")"
        "--model-id" "$(voiceinput_model_id_for_choice "$normalized_model")"
        "--source-url" "$(voiceinput_model_source_url_for_choice "$normalized_model")"
        "--local-dir" "$(voiceinput_model_local_dir_for_choice "$normalized_model")"
      )
      ;;
    *)
      case "$mode" in
        backend)
          VOICEINPUT_EXPANDED_MODEL_ARGS=("--backend" "$normalized_model")
          ;;
        passthrough)
          VOICEINPUT_EXPANDED_MODEL_ARGS=("--model" "$normalized_model")
          ;;
        *)
          echo "不支持的模型展开模式：$mode" >&2
          return 2
          ;;
      esac
      ;;
  esac
}

voiceinput_model_impl() {
  local model=""
  local config_file="$(voiceinput_config_file_path)"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --config-file)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --config-file 的值" >&2
          exit 2
        fi
        config_file="$2"
        shift 2
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b> [--config-file /path/to/voiceinput.env]

说明：
  - 这个命令会把仓库级配置文件写成你选定的默认模型
  - 之后 bootstrap/install/smoke 会默认使用这个模型，除非你再用 --model 覆盖
EOF
        exit 0
        ;;
      *)
        if [[ -z "$model" ]]; then
          model="$1"
          shift
        else
          echo "不支持的参数：$1" >&2
          exit 2
        fi
        ;;
    esac
  done

  if [[ -z "$model" ]]; then
    echo "用法：scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b> [--config-file /path/to/voiceinput.env]" >&2
    exit 2
  fi

  local normalized_model
  if ! normalized_model="$(voiceinput_normalize_model_choice "$model")"; then
    echo "不支持的模型：$model" >&2
    exit 2
  fi

  mkdir -p "$(dirname "$config_file")"
  voiceinput_write_model_config "$normalized_model" "$config_file"
  echo "已写入默认模型：$normalized_model"
  echo "配置文件：$config_file"
}

voiceinput_bootstrap_impl() {
  local -a deploy_args=()
  local smoke_audio_file=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --backend|--model|--model-id|--source-url|--local-dir|--revision|--device|--cuda-wheel-index)
        if [[ $# -lt 2 ]]; then
          echo "缺少 $1 的值" >&2
          exit 2
        fi
        if [[ "$1" == "--model" ]]; then
          if ! voiceinput_expand_model_args backend "$2"; then
            echo "不支持的模型：$2" >&2
            exit 2
          fi
          deploy_args+=("${VOICEINPUT_EXPANDED_MODEL_ARGS[@]}")
        else
          deploy_args+=("$1" "$2")
        fi
        shift 2
        ;;
      --audio-file)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --audio-file 的值" >&2
          exit 2
        fi
        smoke_audio_file="$2"
        shift 2
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh bootstrap [部署参数...] [--audio-file /path/to/audio.wav]

说明：
  - 未传 --audio-file 时，只执行 Python 环境和模型部署
  - 传入 --audio-file 时，会在部署完成后自动运行 Linux smoke
  - 默认会读取 config/voiceinput.env；如果要换文件，可以设置 VOICEINPUT_CONFIG_FILE
  - 部署参数会原样传给 deploy_funasr_model.py

常用部署参数：
  --model funasr|qwen|qwen-0.6b
  --backend funasr|qwen
  --model-id
  --source-url
  --local-dir
  --revision
  --skip-existing
  --install-cuda
  --device auto|cpu|cuda|mps
  --cuda-wheel-index

说明：
  - `--model qwen-0.6b` 会写入 Qwen3-ASR-0.6B 的模型 ID、来源和目录
EOF
        exit 0
        ;;
      *)
        deploy_args+=("$1")
        shift
        ;;
    esac
  done

  voiceinput_configure_rust_mirror
  voiceinput_clear_proxy_env
  export UV_DEFAULT_INDEX="${UV_DEFAULT_INDEX:-https://mirrors.aliyun.com/pypi/simple/}"

  voiceinput_ensure_cargo
  voiceinput_ensure_rustfmt
  voiceinput_ensure_uv

  cd "$REPO_ROOT"

  if [[ -d ".venv" ]]; then
    echo "正在复用 Python 虚拟环境：.venv"
  else
    echo "正在创建 Python 虚拟环境：.venv"
    uv venv .venv --python "$(command -v python3.12)"
  fi

  echo "正在安装模型下载依赖"
  uv pip install -r scripts/requirements-asr-base.txt

  echo "正在安装 ASR 运行时依赖"
  uv pip install -r scripts/requirements-asr-runtime.txt

  echo "正在部署本地 ASR 模型"
  if [[ ${#deploy_args[@]} -gt 0 ]]; then
    uv run -- python scripts/deploy_funasr_model.py --skip-existing "${deploy_args[@]}"
  else
    uv run -- python scripts/deploy_funasr_model.py --skip-existing
  fi

  if [[ -n "$smoke_audio_file" ]]; then
    echo "正在运行 Linux smoke"
    voiceinput_linux_smoke_impl --audio-file "$smoke_audio_file"
  fi

  echo "一键部署完成"
  echo "Rust：$(cargo --version)"
  echo "uv：$(uv --version)"
}

voiceinput_linux_smoke_impl() {
  local audio_file=""
  local backend="ibus"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --audio-file)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --audio-file 的值" >&2
          exit 2
        fi
        audio_file="$2"
        shift 2
        ;;
      --model)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --model 的值" >&2
          exit 2
        fi
        if ! voiceinput_apply_model_choice_env "$2"; then
          echo "不支持的模型：$2" >&2
          exit 2
        fi
        shift 2
        ;;
      --backend)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --backend 的值" >&2
          exit 2
        fi
        backend="$2"
        shift 2
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh linux smoke --audio-file /path/to/audio.wav [--model funasr|qwen|qwen-0.6b] [--backend ibus]

说明：
  - 默认使用 IBus
  - 需要先准备好 Python ASR 环境和本地模型
  - 默认会读取 config/voiceinput.env；如果要换文件，可以设置 VOICEINPUT_CONFIG_FILE
  - `--model` 会通过环境变量传给运行时，`qwen-0.6b` 也可用
EOF
        exit 0
        ;;
      *)
        echo "不支持的参数：$1" >&2
        exit 2
        ;;
    esac
  done

  if [[ -z "$audio_file" ]]; then
    echo "用法：scripts/voiceinput.sh linux smoke --audio-file /path/to/audio.wav [--model funasr|qwen|qwen-0.6b] [--backend ibus]" >&2
    exit 2
  fi

  voiceinput_ensure_cargo
  voiceinput_ensure_uv
  voiceinput_ensure_linux_dev_deps
  voiceinput_refresh_cargo_path
  cd "$REPO_ROOT"
  voiceinput_run_cli_linux smoke --audio-file "$audio_file" --backend "$backend"
}

voiceinput_linux_install_impl() {
  local backend="ibus"
  local audio_file=""
  local run_smoke_after_bootstrap=false
  local run_live_app_after_bootstrap=true
  local setup_autostart=true
  local -a deploy_args=()

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --backend)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --backend 的值" >&2
          exit 2
        fi
        backend="$2"
        shift 2
        ;;
      --audio-file)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --audio-file 的值" >&2
          exit 2
        fi
        audio_file="$2"
        run_smoke_after_bootstrap=true
        run_live_app_after_bootstrap=false
        shift 2
        ;;
      --model)
        if [[ $# -lt 2 ]]; then
          echo "缺少 --model 的值" >&2
          exit 2
        fi
        if ! voiceinput_expand_model_args passthrough "$2"; then
          echo "不支持的模型：$2" >&2
          exit 2
        fi
        deploy_args+=("${VOICEINPUT_EXPANDED_MODEL_ARGS[@]}")
        shift 2
        ;;
      --no-autostart)
        setup_autostart=false
        shift
        ;;
      --no-launch)
        run_live_app_after_bootstrap=false
        shift
        ;;
      --help|-h)
        cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh linux install [--backend ibus] [--model funasr|qwen|qwen-0.6b] [--audio-file /path/to/audio.wav]

说明：
  - 默认先执行 Linux bootstrap，准备 Python 环境并下载模型
  - 会自动安装 Ubuntu 20.04 常用的 Linux 编译依赖，如 pkg-config、libx11-dev、libasound2-dev、portaudio19-dev
  - 然后自动启动 Linux 常驻托盘版
  - 默认会设置 systemd 开机自启，可使用 --no-autostart 跳过
  - 如果传入 --audio-file，会在准备完成后自动跑一次 Linux smoke
  - 默认会读取 config/voiceinput.env；如果要换文件，可以设置 VOICEINPUT_CONFIG_FILE
  - --backend 只影响 Linux 常驻版 / smoke 的宿主后端
  - --model 会原样传给 scripts/voiceinput.sh bootstrap，用来选择 ASR 模型
  - `--model qwen-0.6b` 会切到 Qwen3-ASR-0.6B
EOF
        exit 0
        ;;
      --install-cuda|--skip-existing)
        deploy_args+=("$1")
        shift
        ;;
      --device|--model-id|--local-dir|--revision|--cuda-wheel-index)
        if [[ $# -lt 2 ]]; then
          echo "缺少 $1 的值" >&2
          exit 2
        fi
        deploy_args+=("$1" "$2")
        shift 2
        ;;
      *)
        deploy_args+=("$1")
        shift
        ;;
    esac
  done

  echo "正在准备本地依赖和模型"
  voiceinput_ensure_linux_dev_deps
  if ((${#deploy_args[@]} > 0)); then
    voiceinput_run_bootstrap_args "${deploy_args[@]}"
  else
    voiceinput_run_bootstrap_args
  fi

  if [[ "$run_smoke_after_bootstrap" == true ]]; then
    voiceinput_run_platform_smoke linux "$audio_file" "$backend"
    exit 0
  fi

  if [[ "$run_live_app_after_bootstrap" == true ]]; then
    if [[ "$setup_autostart" == true ]]; then
      voiceinput_setup_linux_autostart "$backend"
    fi
    voiceinput_run_platform_live linux "$backend"
  fi
}

voiceinput_setup_linux_autostart() {
  local backend="${1:-ibus}"

  echo "正在设置 Linux 开机自启..."

  voiceinput_ensure_cargo
  cd "$REPO_ROOT"

  echo "正在编译 release 二进制..."
  cargo build -p voice-input-linux --features ibus --release

  local bin_dir="$HOME/.local/bin"
  local launcher_path="$bin_dir/voice-input"
  mkdir -p "$bin_dir"

  cat > "$launcher_path" <<'LAUNCHER'
#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="__REPO_ROOT__"
cd "$REPO_ROOT"

if [[ -f "$REPO_ROOT/config/voiceinput.env" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$REPO_ROOT/config/voiceinput.env"
  set +a
fi

exec "$REPO_ROOT/target/release/voice-input-linux" live --backend "__BACKEND__"
LAUNCHER

  sed -i "s|__REPO_ROOT__|$REPO_ROOT|g" "$launcher_path"
  sed -i "s|__BACKEND__|$backend|g" "$launcher_path"
  chmod +x "$launcher_path"

  echo "已创建启动脚本：$launcher_path"

  local service_dir="$HOME/.config/systemd/user"
  mkdir -p "$service_dir"

  cat > "$service_dir/voice-input.service" <<SERVICE
[Unit]
Description=VoiceInput 语音输入常驻服务
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=$launcher_path
Restart=on-failure
RestartSec=5

[Install]
WantedBy=graphical-session.target
SERVICE

  systemctl --user daemon-reload
  systemctl --user enable voice-input.service

  echo "已启用 systemd 用户服务：voice-input.service"
  echo ""
  echo "管理命令："
  echo "  启动服务：systemctl --user start voice-input.service"
  echo "  停止服务：systemctl --user stop voice-input.service"
  echo "  查看状态：systemctl --user status voice-input.service"
  echo "  查看日志：journalctl --user -u voice-input.service -f"
  echo "  禁用自启：systemctl --user disable voice-input.service"
}

voiceinput_remove_linux_autostart() {
  echo "正在移除 Linux 开机自启..."

  local service_dir="$HOME/.config/systemd/user"

  if [[ -f "$service_dir/voice-input.service" ]]; then
    systemctl --user stop voice-input.service 2>/dev/null || true
    systemctl --user disable voice-input.service 2>/dev/null || true
    rm -f "$service_dir/voice-input.service"
    systemctl --user daemon-reload
    echo "已移除 systemd 用户服务"
  else
    echo "未找到已安装的 systemd 服务"
  fi

  local launcher_path="$HOME/.local/bin/voice-input"
  if [[ -f "$launcher_path" ]]; then
    rm -f "$launcher_path"
    echo "已移除启动脚本：$launcher_path"
  fi
}

usage() {
  cat >&2 <<'EOF'
用法：
  scripts/voiceinput.sh <command> [args...]

主命令：
  bootstrap              准备 Python 环境、安装依赖并下载模型
  model                  写入仓库级默认模型配置

平台子命令：
  linux install          安装并启动 Linux 常驻版
  linux uninstall        移除 Linux 常驻版及开机自启
  linux smoke            运行 Linux smoke

说明：
  - 所有子命令都会继续兼容现有脚本参数
  - 默认配置来自 config/voiceinput.env
  - 脚本内部会统一转调到 `voice-input-linux`
  - 也可以直接运行 `cargo run -p voice-input-linux --features ibus -- <smoke|live> ...`
EOF
}

cmd="${1:-}"
if [[ -z "$cmd" || "$cmd" == "--help" || "$cmd" == "-h" ]]; then
  usage
  exit 0
fi

shift || true

if [[ "$cmd" == "linux" ]]; then
  platform="$cmd"
  action="${1:-}"
  if [[ -z "$action" || "$action" == "--help" || "$action" == "-h" ]]; then
    usage
    exit 0
  fi
  shift || true
  cmd="${platform}-${action}"
fi

case "$cmd" in
  model)
    voiceinput_model_impl "$@"
    ;;
  bootstrap)
    voiceinput_bootstrap_impl "$@"
    ;;
  linux-install)
    voiceinput_linux_install_impl "$@"
    ;;
  linux-uninstall)
    voiceinput_remove_linux_autostart
    ;;
  linux-smoke)
    voiceinput_linux_smoke_impl "$@"
    ;;
  *)
    echo "不支持的命令：$cmd" >&2
    usage
    exit 2
    ;;
esac
