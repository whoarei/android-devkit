# Android DevKit

[![CI](https://github.com/whoarei/android-devkit/actions/workflows/ci.yml/badge.svg)](https://github.com/whoarei/android-devkit/actions/workflows/ci.yml)

Android DevKit 是一个面向 Windows 的 Android 开发工作台，将 SDK 管理、模拟器、真机连接、APK 安装和日志查看集中在一个桌面应用中。适合准备测试环境、验证应用和排查设备问题，可复用已有 Android SDK 与 AVD。

当前版本：**0.2.1** · [下载发行版](https://github.com/whoarei/android-devkit/releases/latest) · [反馈问题](https://github.com/whoarei/android-devkit/issues)

## 产品特性

- **复用现有开发环境**：接入已有 Android SDK 和模拟器，统一查看和管理设备，减少重复配置。
- **更容易找到需要的 SDK 组件**：按 Android 版本、镜像类型和架构分类查找，结合搜索与安装状态筛选，按需安装或更新组件。
- **为不同测试场景准备模拟器**：选择手机或平板，调整内存、屏幕、摄像头、网络和存储配置，用不同的设备环境验证应用表现。
- **灵活连接真机**：支持 USB、无线调试配对和网络 ADB，根据设备与网络条件选择连接方式，并查看连接与授权状态。
- **明确每次 APK 安装的目标**：选择或拖入 APK，核对目标设备和应用信息后安装；默认保留应用数据，设备断线后不会自动改投其他设备。
- **集中排查设备与应用问题**：过滤实时日志、采集问题发生时的日志片段、截取设备画面，预览并导出排查资料。
- **查进度、找结果、保存文件**：在任务中心搜索历史操作，按状态筛选任务、查看失败原因；通过带角标的待保存文件入口预览、另存或清理缓存副本。
- **按自己的习惯使用界面**：随时切换中英文和亮暗主题，偏好自动保存，下次启动继续使用；支持 `Ctrl` + `+` / `-` 放大或缩小整个界面，`Ctrl` + `0` 恢复默认大小。
- **在应用内检查更新**：启动时静默检查新版本，也可手动查看更新说明；安装版支持验证更新签名后安装并重新打开，便携版提供下载入口。

以下使用说明对应当前源码，发行版的具体功能与变更以对应 Release 说明为准。

## 下载安装

运行环境为 **Windows 11 x64**，需要 WebView2 Runtime。在 [GitHub Releases](https://github.com/whoarei/android-devkit/releases/latest) 选择版本后，可以下载：

- **安装包** `AndroidDevKit-X.Y.Z-setup.exe`：按向导为当前 Windows 用户安装；缺少 WebView2 时，安装器会下载官方引导程序。
- **便携包** `AndroidDevKit-X.Y.Z-windows-x64.zip`：解压后运行 `AndroidDevKit.exe`，保留包内的许可与构建信息文件。

当前发行包未使用 Windows Authenticode 代码签名；应用内更新使用独立的更新包签名校验。Android SDK 组件和系统镜像按需单独下载，使用时需接受对应的 Google / Android 许可。

## 使用方法

### 1. 配置 Android SDK

打开“设置”，选择“当前 SDK 根目录”并保存。已有 Android Studio 环境时，可以选择同一个 SDK 目录；应用的路径配置不修改系统 `PATH`、`JAVA_HOME` 或 Android Studio 设置。

当前默认 SDK 路径是 `E:\Android\sdk`。如果你的 SDK 位于其他位置，或电脑没有 E 盘，请先选择适合的目录。下载遇到网络问题时，可以在设置中配置手动代理。

进入“SDK 管理”并点击“刷新”，按需要选择组件：

- **SDK Platforms**：按 Android API 查找平台与源码；系统镜像按 AOSP、Google APIs、Google Play，再按 ABI 展开。
- **SDK Tools**：查找 Platform-Tools、Emulator、Build-Tools 等工具及其版本。
- **SDK Update Sites**：查看仓库状态，管理自定义更新站点。

使用搜索与状态筛选缩小范围，选择变更后查看计划，确认许可和操作内容再执行。工具或 Java 缺失时，按应用提供的环境准备提示处理；“环境诊断”可检查实际工具链和硬件加速情况。

### 2. 准备模拟器或连接真机

**使用模拟器**：在“设备工作台”点击“新建模拟器”，依次选择设备形态、系统镜像并确认配置。创建后点击“启动”，等待状态变为“可用”。已有 AVD 也会显示在工作台中；模拟器使用 Android 官方独立窗口。

运行模拟器需要可用的硬件虚拟化与加速环境，Windows 配置可参考 [Android Emulator 硬件加速说明](https://developer.android.com/studio/run/emulator-acceleration)。

**使用真机**：

| 连接方式 | 操作方法                                                                                      |
| -------- | --------------------------------------------------------------------------------------------- |
| USB      | 在手机开发者选项中开启 USB 调试，连接数据线，并在手机上允许此电脑调试                         |
| 无线调试 | 点击“连接设备”，填写手机无线调试页面提供的 IP、配对端口和配对码；配对后再使用调试连接端口连接 |
| 网络 ADB | 在“连接设备”中选择“网络 ADB”，输入已开启 TCP/IP 调试服务的设备 IP 和端口，默认端口为 `5555`   |

无线配对端口与调试连接端口可能不同。传统网络 ADB 需要设备已开启 TCP/IP 调试服务；首次开启通常先通过 USB 授权，再使用 SDK 中的 ADB 执行 `adb -s <USB序列号> tcpip 5555`，然后按设备 IP 连接。

设备状态显示“可用”后即可操作。显示“未授权”时，请解锁手机确认授权。点击设备卡片查看详情，再次点击同一卡片取消选中并关闭详情。

### 3. 安装 APK

点击目标设备卡片上的“安装 APK”并选择文件，或将单个 APK 拖到该设备行。在安装面板核对设备名称、序列号和 APK 信息，点击“确认安装计划”，确认后提交任务。

默认覆盖安装并保留应用数据。安装目标会绑定到本次选定的设备连接；设备断开后不会自动改投其他设备。当前支持单个完整 `.apk`，不支持直接安装 AAB 或分包 APK。

安装进度和结果保留在“任务中心”，可按任务名称、设备或状态检索。

### 4. 查看日志、截图与保存文件

选中设备后，在 Logcat 区域点击“开始查看”，按日志级别、Tag、关键词、PID 或应用包名过滤。需要保留一段日志时可启动文件采集；切换页面后采集仍继续，在任务中心停止采集并导出。

在设备详情中点击“截图”，完成后查看或保存截图。任务中心工具栏的“待保存文件”图标通过角标显示数量，点击后可以预览、另存或“忽略并清理”。忽略会删除对应的本地缓存副本，已另存文件、原始采集段和任务记录保留。日志导出默认脱敏，保存前仍可检查预览内容。

标题栏和“设置 → 界面偏好”均可切换语言与主题，选择会自动保存。

### 5. 软件更新

在“设置 → 软件更新”中查看当前版本并点击“检查更新”。发现新版本后可查看更新说明，安装版点击“下载并安装更新”，确认后显示下载、签名校验和安装进度，完成后重新打开应用。安装前需要先完成或停止运行中的任务、结束日志采集，并保存或清理待保存文件。

默认启动时静默检查；可以取消“启动时自动检查更新”，手动检查仍然可用。检查失败时可重试或前往下载页面，更新请求使用设置中的手动代理。便携版和开发版通过下载页手动更新。没有更新入口的旧版本，需要先手动安装包含此功能的新版本。

### 数据存放位置

| 内容                           | 默认位置                                           |
| ------------------------------ | -------------------------------------------------- |
| Android SDK                    | 设置中选择的目录，初始值为 `E:\Android\sdk`        |
| 新建 AVD                       | `%LOCALAPPDATA%\AndroidDevKit\avd`，可在设置中调整 |
| 应用状态、任务、下载缓存与截图 | `%LOCALAPPDATA%\AndroidDevKit`                     |

应用也会发现用户 `.android\avd` 下的已有模拟器。退出工作台与停止模拟器分开；有任务或待保存文件时，按退出面板提示处理。需要关闭窗口后继续在后台运行，可在设置中启用托盘。

## 开发入门

项目使用 **Tauri 2 + React / TypeScript + Rust**：前端负责交互，Rust 通过本机 Android 工具执行 SDK、设备及任务操作。

### 准备开发环境

在 Windows x64 上安装：

- Git 与 Node.js **24.14.0**。
- pnpm **11.25.0**，与 `package.json` 的 `packageManager` 一致。
- Rustup；仓库通过 [rust-toolchain.toml](rust-toolchain.toml) 选择 Rust **1.95.0** 和 Windows MSVC 目标。
- Microsoft C++ Build Tools，勾选“使用 C++ 的桌面开发”并安装 Windows SDK；以及 WebView2 Runtime。安装步骤见 [Tauri Windows 开发前置条件](https://v2.tauri.app/start/prerequisites/#windows)。

编译应用本身不要求先连接真机或准备已有 SDK；调试设备功能时，再按上面的使用方法配置 Android 环境。

### 获取代码并启动

以下命令在 PowerShell 中执行，使用 `pnpm.cmd` 避免 PowerShell 脚本 shim 的执行策略限制。

```powershell
git clone https://github.com/whoarei/android-devkit.git
cd android-devkit
npm install --global pnpm@11.25.0
pnpm.cmd install --frozen-lockfile
pnpm.cmd tauri dev
```

`tauri dev` 会启动 Vite 和桌面窗口，支持开发时重新加载。首次构建需要下载 Rust 依赖并编译，耗时会较长。

只调整界面时，也可以运行 `pnpm.cmd dev`，在浏览器打开 `http://127.0.0.1:19327`。浏览器预览不执行本机 SDK 或设备操作；验证这些功能请使用桌面窗口。开发端口固定为 `19327`，占用时会报错。

### 从哪里开始修改

| 路径                   | 内容                                                                 |
| ---------------------- | -------------------------------------------------------------------- |
| `src/`                 | React 页面、组件、样式、语言与主题；界面入口为 `App.tsx`             |
| `src-tauri/src/`       | Rust 原生逻辑；`commands.rs` 定义 IPC 命令，`domain.rs` 定义数据类型 |
| `src-tauri/resources/` | 工具链与引导下载配置、第三方许可资源                                 |
| `scripts/`             | 构建、打包及验证脚本                                                 |
| `docs/`                | 产品需求、技术方案和历史验证报告                                     |
| `.github/workflows/`   | GitHub CI 与发布流程                                                 |
| `.agents/skills/`      | 项目维护用 skill，包括版本发布流程                                   |

前端通过 `src/api.ts` 调用原生命令。修改 Rust 数据类型后运行 `pnpm.cmd bindings`，更新自动生成的 `src/bindings.ts`，不要直接编辑生成文件。

### 检查与测试

```powershell
# TypeScript、前端构建与 Rust Clippy
pnpm.cmd check

# Rust 测试
pnpm.cmd test

# 检查 Rust 格式
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
```

界面回归使用模拟 IPC，不访问真实设备或 SDK。先构建前端，再运行隔离 Chromium 测试：

```powershell
pnpm.cmd build
pnpm.cmd exec playwright install chromium
$env:PLAYWRIGHT_CHANNEL = 'chromium'
pnpm.cmd test:ui-fixtures
```

真实 SDK / AVD 集成测试按需运行，入口见 [package.json](package.json) 中的 `verify:*` 命令；运行前检查脚本使用的 SDK 和测试目录。测试应用数据可通过 `ANDROID_DEVKIT_DATA_DIR` 隔离，该变量不改变所选 SDK。

### 构建发行文件

```powershell
pnpm.cmd tauri build -- --locked
powershell -NoProfile -File scripts/package-release.ps1
```

完整构建会生成第三方许可清单和 NSIS 安装包，打包后可在 `artifacts/windows/` 找到本地程序，在 `artifacts/release/` 找到安装包、便携 ZIP、许可文件、构建信息和校验和。

以上本地构建不要求更新签名私钥。正式 CI 使用 `tauri.release.conf.json` 和 GitHub Secrets 签名，并通过 `package-release.ps1 -WithUpdater` 额外生成安装器 `.sig` 与 `latest.json` 更新清单；私钥不进入源码或本地构建目录。

[GitHub Actions](https://github.com/whoarei/android-devkit/actions/workflows/ci.yml) 仅在推送 `v*` 标签或在 Actions 页面手动运行时执行检查、测试和 Windows 打包。标签推送在检查通过后自动发布对应版本；手动运行用于检查和打包。维护者的版本确认、标签发布与产物核验步骤见 [版本发布 skill](.agents/skills/android-devkit-release/SKILL.md)。

## 更多资料

- [产品需求与交互方案](docs/00000_20261017232447_Android-DevKit-PRD.html)
- [Tauri 技术方案](docs/00001_20261017234246_Android-DevKit-Technical-Design.html)
- [M3 实现与验证报告](docs/00002_20261018011008_Android-DevKit-M3-Implementation-Report.html)
- [M4 核心功能与内部验收报告](docs/00003_20261018063845_Android-DevKit-M4-Progress-Report.html)

历史报告记录对应阶段的验证范围。提交问题时，请附上应用版本、Windows 版本、连接方式和复现步骤；需要时可从“环境诊断”预览并导出诊断信息。
