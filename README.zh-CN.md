# Claude Account Switcher

[English](README.md) | 简体中文

免安装的 Windows 托盘工具，让多个 Claude 桌面版账号同时运行。
每个账号使用独立的配置目录，切换时不需要重新登录。

面向 **Windows 官方 Claude 桌面版**（已在 4.23118.x 上验证）。

![Claude Account Switcher](dist/preview-light.png)
*截图中账号信息已打码。*

界面预览（使用模拟数据）：[账号搜索与置顶](dist/preview-accounts-3.1.png) · [筛选并批量复制对话](dist/preview-transfer-3.1.png) · [查看已选清单](dist/preview-selection-3.1.1.png)。

两个账号并排运行：

![两个 Claude 账号并排运行](dist/preview-multi.png)

## 功能

- **账号** — 自动列出现有账号的邮箱、运行状态和 Claude Code 对话数。
- **查找账号** — 按 `Ctrl+F` 搜昵称、账号目录名或邮箱，也可勾选「仅运行中」。用 ↑↓ 浏览匹配项，按 Enter 选中，按 Esc 清除筛选。输入多个词可同时匹配不同字段，例如「工作 example.org」。
- **常用账号** — 鼠标移到账号上点「置顶」，它会同时排到侧栏和托盘菜单前面；点「已置顶」即可撤销。右键菜单也可操作。
- **额度** — 5 小时 / 每周用量、重置倒计时，临近重置时给出提示。
- **切换** — 在独立窗口中打开或聚焦账号，并可在账号之间复制 Claude Code 对话。
- **批量复制对话** — 按标题或文件夹搜索，「全选当前结果」只影响匹配项。「查看已选」可核对跨关键词勾选的全部对话，点一条就能移出清单。返回修改或目标尚未就绪时，勾选和目标都会保留；部分复制失败时，只留下失败项供重试。
- **任务栏** — 每个副号独立分组，Claude 图标右下角显示账号头像角标；没有头像时使用首字和账号颜色。已运行的窗口无需重启 Claude 即可更新。请让切换器留在托盘中，以便处理后续新开的窗口。
- **个性化** — 每个账号可单独设置头像和昵称，支持深色 / 浅色主题。
- **删除** — 经确认后永久删除选中的非默认账号目录，包括其中的本地对话；请先备份需要保留的内容。
- **托盘** — 标题栏的 × 只关闭窗口，程序留在通知区；退出在托盘右键菜单里。
- **软件更新** — 从「更多 → 检查更新」、托盘菜单或左下角版本号进入。默认在切换器运行期间按本地日期每天检查一次，同日重启不会重复自动检查；可关闭自动检查，也可随时手动检查。未运行时不会唤醒电脑或创建计划任务。

发现新版本后，点「下载更新」，校验完成再点「替换并重启」。程序会在原位置更新，保留启动项、设置和账号；只重启切换器，不退出 Claude。程序目录需要可写。旧程序保存在同目录的 `.switcher-backup-*/previous.exe`；新版未能启动并完成版本握手时会尝试自动还原。需要手动回退时，退出切换器后将备份复制回原来的程序路径。v3.1.1 及更早版本首次升级仍需从 [GitHub 最新发行版](https://github.com/yuukana1/claude-account-switcher/releases/latest) 下载便携版。

## 运行

`dist/Claude Account Switcher.exe` —— 单文件，无需安装，不依赖 .NET 或 PowerShell。

## 构建

需要 Rust 的 MSVC 工具链，以及包含 C++ 工作负载和 Windows SDK 的 Visual Studio Build Tools。

```powershell
cargo build --release --locked
```

产物：`target\release\claude-account-switcher.exe`。

依赖已缓存时可加 `--offline`；首次构建需要联网下载依赖。

## 开发检查

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

测试使用临时目录和模拟账号数据，不会修改真实 Claude 配置、登录账号、启动 Claude 或修改开机启动项。GitHub Actions 会在 Windows 上执行这些检查并构建可执行文件。

## 数据与安全

启用「开机静默启动」后，登录 Windows 时切换器只驻留托盘，不显示主窗口、不抢占焦点。已启用的旧启动项会迁移到 `--background` 模式；原本关闭自启的安装不会自动开启。手动运行程序或点击托盘图标仍可打开主窗口。

如果 Claude 更新后所有账号都无法启动，先确认开始菜单里的 Claude 能否打开。切换器会重新查询当前用户注册的 Store 版本，并显示安装状态和 Windows 启动错误。可运行 `& '.\Claude Account Switcher.exe' --diagnose-install` 查看检测到的版本、程序位置和安装状态；该命令不读取账号内容。若系统安装日志提示 `0x80070430`（服务已标记为删除），保存工作后重启 Windows，再完成 Claude 更新；无需删除或重新添加账号。

账号数据在本机读取。更新检查通过 HTTPS 访问本项目的 GitHub Releases；用户选择下载时，会检查 Windows x64 附件的大小和 GitHub 提供的 SHA-256。请求不包含账号、邮箱、对话或登录凭据。添加账号会创建独立配置目录并启动 Claude。主动发起的对话复制会写入新的索引文件，不覆盖已存在的副本，也不修改源文件。确认删除后，会永久移除选中的非默认账号目录及其中的本地对话；默认账号不能删除。

切换器自己的设置和头像会先完整写入临时文件，再替换目标文件。保存失败会提示错误并保留原文件。已有设置文件无法读取或解析时，程序会保留它；请修复文件后重新启动，再保存设置。

启动时还会检查当前用户 Windows“启动”目录中遗留的 `Claude Profile Switcher.lnk.disabled`。若它是普通的快捷方式文件，会将其移到 `%LOCALAPPDATA%\ClaudeProfiles\.switcher\startup-backups`，完整保留内容且不覆盖已有备份。停用的快捷方式应放在“启动”目录之外，避免 Windows 弹出文件打开方式。启用中的快捷方式及其他程序的启动项不迁移。

Anthropic Sans/Serif 字体从本机已安装的 Claude 中提取，不随程序分发。

## 致谢

基于 [Claude Code Desktop Switcher](https://github.com/PriyanshuGeTRekT/Claude-Code-Desktop-Switcher)（MIT 协议）重写。见 [LICENSE](LICENSE)。
