# Android DevKit

[![CI](https://github.com/whoarei/android-devkit/actions/workflows/ci.yml/badge.svg)](https://github.com/whoarei/android-devkit/actions/workflows/ci.yml)

Android DevKit 是一个面向 Windows 的 Android 开发工作台，将 SDK 管理、模拟器、真机连接、APK 安装和日志查看集中在一个桌面应用中。适合准备测试环境、验证应用和排查设备问题，可复用已有 Android SDK 与 AVD。

当前版本：**0.5.0** · [下载发行版](https://github.com/whoarei/android-devkit/releases/latest) · [反馈问题](https://github.com/whoarei/android-devkit/issues)

一期已按 2026-10-24 调整后的范围完成：现有功能及本轮故障专项已验证，v0.4.0 已发布，其余验收事项继续保留 TODO，保留在 [一期 TODO 与完成范围](docs/00008_20261024164440_Android-DevKit-Phase1-TODO.html)。

0.4.0 完成一期功能收尾：新增 SDK 发现与复用、Java 环境检测与私有运行时配置、系统镜像按 API 分组、任务记录删除与采集清理、默认设备与就绪后安装、环境摘要复制及 AVD 空间统计；修复多模拟器目标歧义和升级安装时旧进程尚未退出的问题，并补充 35 场景故障专项验证。已有 SDK / Java 路径保持沿用。

0.5.0 新增 **分包 APK 安装 → 录屏 → 文件传输**，包含明确的冲突选择、完整暂存后发布与中断原件恢复；修复重复提交、错误反馈、原件保护、录屏恢复、缓存回收及键盘焦点等 18 组问题。自动验证与实体手机待验项见 [二期实施与验收报告](docs/00010_20261024234040_Android-DevKit-Phase2-Implementation-Report.html) 和 [循环审核与修复报告](docs/00011_20261025010136_Android-DevKit-Phase2-Review-Report.html)。

## 产品特性

- **复用现有开发环境**：接入已有 Android SDK 和模拟器，统一查看和管理设备，减少重复配置。
- **更容易找到需要的 SDK 组件**：按 Android 版本、镜像类型和架构分类查找，结合搜索与安装状态筛选，按需安装或更新组件。
- **按网络条件切换 SDK 下载源**：选择 Google 官方、腾讯云镜像或自定义 HTTPS 根地址，检查后统一切换目录与归档下载；官方校验记录可缓存复用，也可按需关闭官方校验。
- **为不同测试场景准备模拟器**：选择手机或平板，调整内存、屏幕、摄像头、网络和存储配置，用不同的设备环境验证应用表现。
- **灵活连接真机**：支持 USB、无线调试配对和网络 ADB，根据设备与网络条件选择连接方式，并查看连接与授权状态。
- **明确每次 APK 安装的目标**：选择或拖入 APK，核对目标设备和应用信息后安装；默认保留应用数据，设备断线后不会自动改投其他设备。
- **一次安装整组分包 APK**：多选 base / split APK 或选择目录，预先校验包名、版本、签名、设备兼容性与依赖，再提交安装。
- **录屏后预览与保存**：支持 1–180 秒无声 MP4、提前停止和中断后取回；完整缓存的视频会在重启后恢复到待保存文件。
- **在共享存储传输文件**：浏览 Download、Pictures、Movies、DCIM，上传或下载文件与目录，明确处理同名冲突，替换中断后可恢复原件。
- **集中排查设备与应用问题**：过滤实时日志、采集问题发生时的日志片段、截取设备画面，预览并导出排查资料。
- **直接操作交互终端**：从设备详情进入对应的 ADB Shell，也可打开本机 PowerShell；支持多会话、实时交互、切页保留输出和直接关闭会话。
- **查进度、找结果、保存文件**：在任务中心搜索历史操作，按状态筛选任务、查看失败原因；通过带角标的待保存文件入口预览、另存或清理缓存副本。
- **按自己的习惯使用界面**：随时切换中英文和亮暗主题，偏好自动保存，下次启动继续使用；支持 `Ctrl` + `+` / `-` 放大或缩小整个界面，`Ctrl` + `0` 恢复默认大小。
- **在应用内检查更新**：启动时静默检查新版本，也可手动查看更新说明；安装版支持验证更新签名后安装并重新打开，便携版提供下载入口。

以下使用说明对应当前源码，发行版的具体功能与变更以对应 Release 说明为准。

## 下载安装

运行环境为 **Windows 11 x64**，需要 WebView2 Runtime。在 [GitHub Releases](https://github.com/whoarei/android-devkit/releases/latest) 选择版本后，可以下载：

- **安装包** [AndroidDevKit-0.5.0-setup.exe](https://github.com/whoarei/android-devkit/releases/download/v0.5.0/AndroidDevKit-0.5.0-setup.exe)：按向导为当前 Windows 用户安装；缺少 WebView2 时，安装器会下载官方引导程序。
- **便携包** [AndroidDevKit-0.5.0-windows-x64.zip](https://github.com/whoarei/android-devkit/releases/download/v0.5.0/AndroidDevKit-0.5.0-windows-x64.zip)：解压后运行 `AndroidDevKit.exe`，保留包内的许可与构建信息文件。

当前发行包未使用 Windows Authenticode 代码签名；应用内更新使用独立的更新包签名校验。Android SDK 组件和系统镜像按需单独下载，使用时需接受对应的 Google / Android 许可。

## 使用方法

### 1. 配置 Android SDK

首次使用会发现当前配置、Android 环境变量和标准用户目录下的 SDK，显示来源、工具路径、可写状态和可用空间。选择“复用已有环境”后应用所选目录；也可以按用途准备真机或模拟器的最小环境，安装前仍需确认计划与许可。之后可从“设置 → 发现并选择 SDK 环境”重新选择，或手动保存“当前 SDK 根目录”。应用不修改系统 PATH、JAVA_HOME 或 Android Studio 配置。

新配置默认使用当前用户的 `%LOCALAPPDATA%\Android\Sdk`，已保存的 SDK 路径继续沿用；环境发现仍会检查 `E:\Android\sdk`。多个目录由用户明确选择，不自动合并或迁移。下载遇到网络问题时，可以在设置中配置手动代理。

进入“SDK 管理”并点击“刷新”，按需要选择组件：

- **SDK Platforms**：按 Android API 查找平台与源码；系统镜像按 AOSP、Google APIs、Google Play，再按 ABI 展开。
- **SDK Tools**：查找 Platform-Tools、Emulator、Build-Tools 等工具及其版本。
- **下载源与站点**：选择 Google 官方、腾讯云镜像或自定义 HTTPS 仓库根地址，点击“检查连接”后“应用并刷新”；已有 SDK 安装任务结束后才能切换。自定义源应保留 Android 官方目录结构，第三方附加站点仍使用自身地址。

“使用 Google 官方校验值验证安装包”默认开启。官方暂不可达时可使用已保存的官方记录；所选版本缺少匹配记录时暂缓安装。关闭此选项后仍按下载源提供的校验值验证完整性，任务记录会注明校验方式。新安装或升级后先刷新目录，以取得归档与校验元数据。

使用搜索与状态筛选缩小范围，选择变更后查看计划，确认许可和操作内容再执行。SDK 管理、设置和环境诊断均提供“配置 Java”：可一键准备应用私有 Temurin JRE 21，或检测并复用已有 JDK、JRE、Android Studio 自带运行时。SDK 安装前会检查 Java 启动及工具兼容性；配置完成后保留组件选择，返回原安装计划再次确认。

私有 Java 安装在 `%LOCALAPPDATA%\AndroidDevKit\runtime\java21`，仅供本应用 SDK 工具使用，不修改系统环境变量。Java 下载使用 Adoptium 官方发行地址，沿用手动代理，独立于 SDK 镜像。准备进度、取消和失败记录可在任务中心查看；SDK 工具缺失时仍需使用“准备模拟器环境”。

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

设备列表支持方向键、Home / End 切换和 Escape 取消选择。详情可复制设备标识、AVD 完整数据路径，并将设备设为默认目标；未选择查看目标时，APK 面板使用当前可识别的默认目标。默认设备不在线时仍要求明确选择，不自动改投。AVD 实际占用按需统计，排除共享镜像、外部存储和链接；删除计划展示预计可释放空间。

启动超过提醒阈值时模拟器继续运行，可从详情或原启动任务选择“继续等待系统就绪”。安装面板可选择“就绪后安装 APK”，最多等待设置中的阈值，可在任务中心取消；等待不会启动新实例，已识别连接断开或 ADB 会话变化后不会改投其他设备。

### 3. 安装 APK

点击目标设备卡片上的“安装 APK”并选择文件，或将单个 APK 拖到该设备行。在安装面板核对设备名称、序列号和 APK 信息，点击“确认安装计划”，确认后提交任务。

默认覆盖安装并保留应用数据。安装目标会绑定到本次选定的设备连接；设备断开后不会自动改投其他设备。支持单个完整 `.apk`，也可从设备详情进入“分包 APK 安装”，多选 base / split APK 或选择目录，校验签名、版本与依赖后一次安装；AAB、APKS、APKM、XAPK 容器暂不支持直接导入。

安装进度和结果保留在“任务中心”，可按任务名称、设备或状态检索。

任务条目右侧可展开详情、重试或删除记录；删除点击即执行。通过“管理任务”勾选本页任务批量删除，或用“清理历史”处理当前搜索与状态下的全部分页。进行中、收尾与恢复中的任务受保护；有待保存文件或原始采集时先处理文件。“清理历史”还会保留待检查任务。删除记录不撤销执行结果，也不删除已另存文件。

### 4. 查看日志、截图与保存文件

选中设备后，在 Logcat 区域点击“开始查看”，按日志级别、Tag、关键词、PID 或应用包名过滤。需要保留一段日志时可启动文件采集；切换页面后采集仍继续，在任务中心停止采集并导出。

在设备详情中点击“截图”，完成后查看或保存截图。任务中心工具栏的“待保存文件”图标通过角标显示数量，点击后可以预览、另存或“忽略并清理”。忽略会删除对应的本地缓存副本，已另存文件、原始采集段和任务记录保留。日志导出默认脱敏，保存前仍可检查预览内容。

“环境诊断 → 预览并复制环境摘要”默认脱敏，核对预览后可直接复制，不产生待保存导出副本；需要原始设备标识时先显式勾选包含原始标识。

标题栏和“设置 → 界面偏好”均可切换语言与主题，选择会自动保存。

### 5. 使用 ADB 终端

选中设备，在详情操作区点击“ADB 终端”，直接进入该设备的交互 Shell，可输入 `getprop`、`logcat`、`top` 等设备命令。左侧“ADB 终端”页的“新建会话”菜单可直接选择设备，或打开已配置当前 SDK 的本机 PowerShell；通用命令行中使用 `adb devices -l` 查看设备、通过 `adb -s <序列号> shell` 指定目标。

终端支持连续输入、方向键、Tab 补全、复制粘贴和 `Ctrl+C` 中断，命令直接作用于实际环境。常用命令入口只插入文本，执行由终端处理。切换页面保留会话和输出；点击会话上的 × 直接关闭该会话。设备 Shell 退出或断线后保留输出，设备恢复后可重新连接。终端命令的结果在对应会话中查看；实际退出应用会关闭终端连接，隐藏到托盘则保留会话。

每个连接使用创建时的 SDK 环境，设置切换只影响新连接。替换或卸载会话正在使用的 Platform-Tools、安装软件更新前，需要先关闭相关终端会话。

### 6. 软件更新

在“设置 → 软件更新”中查看当前版本并点击“检查更新”。发现新版本后可查看更新说明，安装版点击“下载并安装更新”，确认后显示下载、签名校验和安装进度，完成后重新打开应用。安装前需要先完成或停止运行中的任务、结束日志采集，并保存或清理待保存文件。

默认启动时静默检查；可以取消“启动时自动检查更新”，手动检查仍然可用。检查失败时可重试或前往下载页面，更新请求使用设置中的手动代理。便携版和开发版通过下载页手动更新。没有更新入口的旧版本，需要先手动安装包含此功能的新版本。

若 Windows 安装器提示“无法终止 Android DevKit。请关闭后再试”，先取消安装，保存工作并完全退出 Android DevKit 后，再次运行安装包。启用托盘时，关闭窗口可能只会隐藏到托盘，需要从托盘菜单选择“退出…”；仍有残留时可在任务管理器结束 `android-devkit.exe` 后重试。更新安装器会先等待旧进程释放文件，最长 20 秒，再执行占用检查。

### 数据存放位置

| 内容                           | 默认位置                                                    |
| ------------------------------ | ----------------------------------------------------------- |
| Android SDK                    | 设置中选择的目录，新配置默认为 `%LOCALAPPDATA%\Android\Sdk` |
| 新建 AVD                       | `%LOCALAPPDATA%\AndroidDevKit\avd`，可在设置中调整          |
| 应用状态、任务、下载缓存与截图 | `%LOCALAPPDATA%\AndroidDevKit`                              |

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

### 二期设备操作

选择可用设备后，详情提供“分包 APK 安装”“录屏”“文件传输”。录屏默认 30 秒，可选 1–180 秒，停止后完整取回至待保存文件；可预览、另存或清理缓存。中断后选择“取回原录屏”，不会自动重录；已完整缓存的 MP4 会在重启后恢复到待保存文件。也可通过明确的确认计划放弃设备原件，保留已经取回的本机视频。

文件传输支持共享存储浏览、多选文件或目录、拖入上传；同名目标必须明确选择保留、重命名或替换。先写入任务专属暂存，校验后发布；替换中断会保留旧目标备份，可从文件面板恢复，普通清理不会删除受保护原件。传输只访问 Download、Pictures、Movies、DCIM，不跟随链接或绕过私有目录。

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

故障专项可独立复现，包含 35 个场景、14 次测试子进程强杀，报告写入 `.qa/faults/run-*/report.json`：

```powershell
pnpm.cmd verify:faults
```

专项使用临时目录、本地 HTTP 响应、测试专用 I/O 故障点、SQLite 页数限制 / 只读连接和 Windows 文件锁。SDK 覆盖真实引导归档提交与多组件清单复查；APK 使用隔离响应夹具验证结果落库前的中断恢复。测试不操作用户设备或已选 SDK，进程终止也不等同于断电验收。

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

- [二期循环审核与修复报告](docs/00011_20261025010136_Android-DevKit-Phase2-Review-Report.html)

- [二期范围与验收基线：真机使用效率](docs/00009_20261024174538_Android-DevKit-Phase2-Scope.html)
- [二期实施、自动验证与实机待验报告](docs/00010_20261024234040_Android-DevKit-Phase2-Implementation-Report.html)
- [一期 TODO 与完成范围](docs/00008_20261024164440_Android-DevKit-Phase1-TODO.html)
- [一期功能收口与验收记录](docs/00007_20261024162627_Android-DevKit-Phase1-Completion-Report.html)
- [产品需求与交互方案](docs/00000_20261017232447_Android-DevKit-PRD.html)
- [Tauri 技术方案](docs/00001_20261017234246_Android-DevKit-Technical-Design.html)
- [M3 实现与验证报告](docs/00002_20261018011008_Android-DevKit-M3-Implementation-Report.html)
- [M4 核心功能与内部验收报告](docs/00003_20261018063845_Android-DevKit-M4-Progress-Report.html)
- [SDK 下载源切换交互原型](docs/00004_20261024112652_Android-DevKit-SDK-Mirror-Prototype.html)
- [Java 环境配置交互方案](docs/00005_20261024144951_Android-DevKit-Java-Environment-Design.html)
- [任务删除交互方案](docs/00006_20261024145638_Android-DevKit-Task-Deletion-Design.html)

历史报告记录对应阶段的验证范围。提交问题时，请附上应用版本、Windows 版本、连接方式和复现步骤；需要时可从“环境诊断”预览并导出诊断信息。

二期隔离 SDK 验证（需要既有 E:\Android\sdk、API 29 default x86_64 镜像、Build-Tools 35 与 Android Studio JBR）：

```powershell
pnpm.cmd verify:phase2
# 单独复验替换中断后的原件保护与恢复（仅一台专用 AVD）
pnpm.cmd verify:phase2-recovery
```

该入口仅在 .qa/runtime 创建两台专用 AVD，验证分包升级与数据保留、录屏与取回、文件往返及提交点强杀恢复，并清理自己的设备实例。实际手机 USB / 无线和系统文件对话框仍按验收报告人工登记。
