# Windows 桌面交互回归

`desktop/scripts/explore-windows.ps1` 安装真实 NSIS 安装包，通过 Windows UI Automation 查找按钮并执行点击、键盘输入和滚动。测试先最大化窗口，避免默认窗口超出测试机屏幕、鼠标误点任务栏。测试使用独立的临时 `FUSION_DATA_DIR`，不读取或修改用户数据，也不配置或调用付费模型。

## 已复现的问题

[v0.1.4 基线检查](https://github.com/caixiaoshun/rongguang-desktop/actions/runs/41594680490)的截图和可访问树确认：

| 操作 | 原行为 | 修复 |
| --- | --- | --- |
| 点击 GitHub | `Command plugin:shell\|open not allowed by ACL` | 桌面外链统一调用 `open_url`，交给系统浏览器；为当次后端端口的确切 loopback 来源授予四个应用命令权限；修正目标仓库地址 |
| 打开助手 | `Cannot read properties of undefined (reading 'map')` | 后端提供 `skills` / `mcpTools` 数组；前端兼容旧响应 |
| 读取会话历史或任务中心 | 前端需要 `{list,total}`，后端返回数组 | 统一分页响应、参数和分类筛选；保留无分类的旧助手历史 |
| 托盘打开数据目录 | 使用 Tauri identifier 目录，与 SQLite / 媒体目录不同 | 使用后端实际解析的数据目录，包括环境变量覆盖 |

基线脚本的错误匹配未包含 shell ACL 提示，因此 GitHub 一行被误标为通过；该错误由保存的界面证据发现，现已纳入失败断言。基线的首次首页断言也存在页面尚未就绪的时序问题，现已改为等待期望内容。

## 覆盖范围

| 区域 | 操作 |
| --- | --- |
| 顶部与仪表盘 | 主题往返切换、任务中心开关、用户菜单、个人设置入口、总览/数据分析 |
| 项目 | 列表、新建/取消、填写并提交项目、概览/剧本/分镜/资产/设置、保存项目改名并还原、删除确认后取消 |
| 工具与资产 | 全局素材资产、生图与生视频工作台入口 |
| 设置 | 六个侧栏页面；API、存储、Skill、MCP 弹窗；创建并编辑测试 Skill；Release 外链 |
| 助手 | 打开、历史列表、`@` 项目选择器、`/` Skill 选择器、清空输入、最大化/恢复、关闭/重开 |
| 外链 | GitHub 支持鼠标、键盘和辅助功能激活；GitHub 和 Release 链接均要求原生浏览器命令返回成功，并检查界面无 ACL / 异步异常提示 |

每项记录结果、界面可访问树和截图。构建工作流在 Windows 原有的重复启动、响应性、退出和端口冲突检查后，运行完整交互回归；失败时仍上传 `desktop-interaction-evidence` 和启动诊断。

未覆盖真实模型输出、付费图片/视频生成、第三方凭据有效性、远程 MCP 执行、生产存储迁移和自动升级覆盖安装。这些操作需要单独的测试凭据、服务或版本条件，不能由按钮检查通过推断为可用。外链检查确认系统接受打开请求，不证明远程网站的网络加载成功。

## 本地复现

在有桌面会话、WebView2 和 Windows PowerShell 的机器上，使用待验证构建的安装包和新输出目录执行。已发布的原版 v0.1.4 是已知失败基线，与同名的测试构建应分开：

```powershell
./desktop/scripts/explore-windows.ps1 -Installer 'C:\Downloads\RongGuang_0.1.4_x64-setup.exe' -OutputDir "$env:TEMP\rongguang-interactions-test"
```

检查 `evidence/results.json`、相邻截图 / JSON 和 `evidence/logs/`。输出目录应为新目录，以免旧数据、日志和重复安装影响断言。
