#!/usr/bin/env bash
#
# 融光桌面端构建脚本（Linux / macOS 通用，Windows 请用 build-windows.ps1）。
#
# 产物结构（dist/<platform>/）：
#   RongGuang(.exe)        主程序
#   web/                   前端静态资源
#   bin/ffmpeg bin/ffprobe 随包媒体工具
#
# 用法：
#   ./scripts/build.sh              # 构建当前平台
#   ./scripts/build.sh --skip-web   # 复用已有前端产物
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_DIR="$ROOT/../web"
DIST="$ROOT/dist/$(uname -s | tr '[:upper:]' '[:lower:]')"
SKIP_WEB=0

for arg in "$@"; do
  case "$arg" in
    --skip-web) SKIP_WEB=1 ;;
    *) echo "未知参数: $arg" >&2; exit 1 ;;
  esac
done

echo "==> 仓库根: $ROOT"

# 1. 前端静态导出
if [ "$SKIP_WEB" -eq 0 ]; then
  echo "==> 构建前端（静态导出）"
  cd "$WEB_DIR"
  if [ -d node_modules ]; then
    pnpm install --prefer-offline
  else
    pnpm install
  fi
  FUSION_DESKTOP_BUILD=1 pnpm build
fi

if [ ! -f "$WEB_DIR/out/index.html" ]; then
  echo "!! 前端产物缺失: $WEB_DIR/out/index.html" >&2
  exit 1
fi

# 2. Rust 主程序
echo "==> 编译 Rust（release）"
cd "$ROOT"
cargo build --release -p rongguang

BIN="target/release/rongguang"
[ -f "$BIN" ] || { echo "!! 未找到可执行文件 $BIN" >&2; exit 1; }

# 3. 组装发布目录
echo "==> 组装 $DIST"
rm -rf "$DIST"
mkdir -p "$DIST/bin"
cp "$BIN" "$DIST/"
cp -R "$WEB_DIR/out" "$DIST/web"

# 4. 随包 ffmpeg：必须用**静态链接**构建。
#    系统 PATH 里的 ffmpeg 通常是动态链接的（Linux 下动辄 160MB 且依赖一堆 .so），
#    直接拷进发布包既臃肿又换机器就崩。优先级：
#    FFMPEG_BIN_DIR（自备静态包目录） > vendor/ffmpeg/<platform>/ > 系统 PATH（仅告警）
PLATFORM="$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"
VENDOR_DIR="$ROOT/vendor/ffmpeg/$PLATFORM"

copy_tools() {
  local src="$1"
  for tool in ffmpeg ffprobe; do
    if [ -x "$src/$tool" ]; then
      cp -f "$src/$tool" "$DIST/bin/"
    else
      echo "!! $src 缺少 $tool" >&2
      return 1
    fi
  done
}

if [ -n "${FFMPEG_BIN_DIR:-}" ] && copy_tools "$FFMPEG_BIN_DIR"; then
  echo "==> ffmpeg 来自 FFMPEG_BIN_DIR=$FFMPEG_BIN_DIR"
elif [ -d "$ROOT/vendor/ffmpeg/current" ] && copy_tools "$ROOT/vendor/ffmpeg/current"; then
  # 与 GitHub Actions 保持同一约定：CI 把下载的静态二进制放在 current/
  echo "==> ffmpeg 来自 vendor/ffmpeg/current"
elif [ -d "$VENDOR_DIR" ] && copy_tools "$VENDOR_DIR"; then
  echo "==> ffmpeg 来自 vendor 目录 $VENDOR_DIR"
else
  echo "!! 未找到静态 ffmpeg，回落到系统 PATH（体积大且不可移植，仅供本地自测）" >&2
  for tool in ffmpeg ffprobe; do
    if command -v "$tool" >/dev/null 2>&1; then
      cp -f "$(command -v "$tool")" "$DIST/bin/"
    else
      echo "!! 未找到 $tool，请设置 FFMPEG_BIN_DIR 指向预编译静态 ffmpeg 目录" >&2
    fi
  done
fi

# ffmpeg 体积体检：静态构建单文件通常 70–90MB，动态构建常见 150MB+
for tool in ffmpeg ffprobe; do
  f="$DIST/bin/$tool"
  if [ -f "$f" ]; then
    size_mb=$(( $(stat -c%s "$f") / 1024 / 1024 ))
    if [ "$size_mb" -gt 130 ]; then
      echo "!! $tool 体积 ${size_mb}MB，疑似动态链接构建，发布包不可移植" >&2
    else
      echo "==> $tool ${size_mb}MB"
    fi
  fi
done

echo
echo "==> 完成"
du -sh "$DIST" "$DIST/web" "$DIST/bin" 2>/dev/null || true
echo "启动: $DIST/rongguang"
