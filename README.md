# 融光 · AI 视频创作工作台（桌面版）

Agent 驱动的视频创作桌面应用。本地运行、单机单用户、数据全在本机，默认接入 Grok 全模态模型。

> 本仓库是原 [ai-fusion-video](https://github.com/Stonewuu/ai-fusion-video)（Java / Spring Boot + Next.js + MySQL + Redis）
> 的 **桌面端重写版**：后端以 Rust 重写，打包为 Windows / macOS / Linux 原生应用，前端界面与功能完整保留。

## 两个托管平台，同一份代码

代码在 **GitHub** 与 **Gitee** 双向开源，安装包两边都发布，你可以挑访问顺畅的一个：

| 平台 | 仓库 | 安装包 |
| --- | --- | --- |
| **GitHub**（主） | <https://github.com/caixiaoshun/rongguang-desktop> | <https://github.com/caixiaoshun/rongguang-desktop/releases> |
| **Gitee**（国内镜像） | <https://gitee.com/caixiaoshun/ai-fusion-video> | <https://gitee.com/caixiaoshun/ai-fusion-video/releases> |

两边内容一致：同一套源码、同一个 tag、同三个平台的安装包。国内访问 GitHub 慢就从 Gitee 下，
GitHub 打不开也还有 Gitee 兜着。应用内的「检查更新」会优先查 Gitee，连不上自动回退 GitHub。

Issue / PR 提在哪边都可以，看到都会处理。


## 特性

- **全模态创作**：文本对话、图像生成、视频生成、语音合成、语音转写
- **Agent 工作流**：智能体编排剧本 → 分镜 → 素材，支持多轮工具调用
- **单机自持**：SQLite 单文件 + 本地媒体库，无需 MySQL / Redis / Docker
- **默认 Grok**：内置 Grok 供应商槽位，填入服务地址与 Key 即可使用；其余供应商按需启用
- **媒体本地化**：生成的图片/视频/音频全部落到本机，不依赖上游留存

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面壳 | Tauri 2（WebView 指向本地 `127.0.0.1`，前后端同源） |
| 后端 | Rust / axum 0.8 / sqlx 0.8 (SQLite) / tokio / reqwest |
| 前端 | Next.js 16（静态导出）/ React 19 / Tailwind 4 / Ant Design X |
| 存储 | SQLite 单文件 + 本地媒体目录 |
| 媒体处理 | 随包 ffmpeg（抽帧、拼接） |

## 目录结构

```
.
├── desktop/                 Rust 工作区
│   ├── crates/core/         配置、路径、错误、数据模型
│   ├── crates/ai/           供应商协议层（Grok / OpenAI 兼容）与媒体落地
│   ├── crates/server/       HTTP 服务：路由、业务逻辑、SQLite 迁移
│   ├── src-tauri/           桌面壳：进程内拉起服务、窗口、托盘
│   └── scripts/             跨平台打包脚本
└── web/                     前端（Next.js 静态导出）
```

## 开发

前置：Rust stable、Node.js 20+、pnpm。

```bash
# 1. 前端依赖
cd web && pnpm install

# 2. 后端（仅服务，无需 GUI，便于调试接口）
cd ../desktop
FUSION_DATA_DIR=./data cargo run -p fusion-server

# 3. 桌面端（需要系统 WebView 依赖）
cargo run -p rongguang
```

接口契约保持 `{ code, msg, data }`，业务错误返回 HTTP 200 且 `code != 0`。

## 构建

### 下载现成安装包

CI 会自动构建并同时发布到 GitHub 和 Gitee（见文首的仓库对照表），两边产物一致。

| 平台 | 产物 |
| --- | --- |
| Windows | `RongGuang_<版本>_x64-setup.exe` — NSIS 安装包，双击安装、自带卸载 |
| Linux | `RongGuang_<版本>_amd64.deb` |
| macOS | `RongGuang_<版本>_aarch64.dmg` |

每个平台只发一个安装包。macOS **只提供 Apple Silicon（arm64）**版本，Intel 机器请自行从源码构建。

> ⚠️ **安装包既不内置 API Key，也不预置任何服务地址**。仓库和产物都是公开分发的，
> 所以这两项一律由用户自己填：首次启动后到「设置 → AI 模型」里，把 **服务地址** 和 **API Key**
> 都填上才能保存。首页会给出提示，字段空着时保存按钮是禁用的。

### 检查更新

应用内「设置 → 通用设置 → 版本更新」可以检查新版本：

- 检测源优先 **Gitee**，连不上会自动回退 **GitHub**
- **更新是可选的**：发现新版本后可以「下载并安装」，也可以点「暂不更新」跳过这个版本；
  跳过只记住版本号，不自动下载、不影响使用，下个版本发布时会重新提示，随时能「恢复更新提醒」
- Windows 会启动 `setup.exe`；macOS 会打开 dmg，拖进「应用程序」覆盖即可

推送 `v*` 形式的 tag，或在 Actions 页面手动触发 `Build Desktop` 并勾选发布，即可产出新版本。

### 发版流程（维护者）

CI 负责「构建 + 发 GitHub Release + 在 Gitee 建好 Release 容器」，**Gitee 的附件不经过 CI 上传**：
实测 GitHub runner 到 Gitee 单连接只有约 0.2MB/s，三个包传不完，所以由维护者本机补传。

```bash
# 1. 推 tag，等 CI 三个平台构建完、GitHub Release 发布好
git tag v0.1.3 && git push gh v0.1.3

# 2. 把安装包下到本地某个目录，然后补传 Gitee
GITEE_TOKEN=xxxxx ./scripts/gitee-upload-release.sh v0.1.3 dist/
```

脚本会先删掉同名旧附件（Gitee 重复上传会堆记录、不覆盖），再逐个上传并回读核对。

### Linux / macOS

```bash
cd desktop
./scripts/build.sh                 # 前端导出 + Rust 编译 + 组装 dist/
./scripts/build.sh --skip-web      # 复用已有前端产物
```

### Windows

```powershell
cd desktop
powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1
```

### 随包 ffmpeg

发布包需要**静态链接**的 ffmpeg，否则换机器会因缺少动态库直接崩溃。
打包脚本按以下顺序查找，命中即用：

```
desktop/vendor/ffmpeg/current/ffmpeg.exe   # CI 下载到这里，优先级最高
desktop/vendor/ffmpeg/ffmpeg.exe
```

Linux / macOS 把 `.exe` 后缀去掉即可。也可以用环境变量 `FFMPEG_BIN_DIR`
指向任意静态构建目录。静态构建单文件通常 70–90MB，若超过 130MB 说明是动态构建，脚本会告警。

## 配置

首次启动会在用户数据目录生成 `config.json`，可用环境变量覆盖：

| 变量 | 说明 |
|---|---|
| `FUSION_DATA_DIR` | 数据根目录（默认随平台） |
| `FUSION_PORT` | 服务端口（默认随机空闲端口） |
| `FUSION_WEB_DIR` | 前端静态资源目录 |
| `FUSION_GROK_KEY` | Grok API Key（可选，用于自建部署；普通用户直接在设置页填） |

数据目录内容：`fusion.db`、`media/`、`uploads/`、`workspace/`、`logs/`。

## 许可证

MIT。原项目 Copyright (c) 2026 Stone Wu，桌面端改造部分同样以 MIT 发布。
