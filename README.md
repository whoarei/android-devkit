# Android DevKit

[![CI](https://github.com/whoarei/android-devkit/actions/workflows/ci.yml/badge.svg)](https://github.com/whoarei/android-devkit/actions/workflows/ci.yml)

基于 Tauri 2、React / TypeScript 和 Rust 的 Windows Android 开发工作台，当前发布版本 `0.1.0`。支持 SDK、模拟器、真机连接、APK、日志与任务管理，以及中英文和亮暗主题切换。完整 P0 / 发布验收仍有待补充项。

默认使用 **`E:\Android\sdk`**。在设置中可以明确切换 SDK；发现其他 SDK 不会自动替换当前选择。模拟器使用 Android 官方独立窗口。

## 启动

从 [GitHub Releases](https://github.com/whoarei/android-devkit/releases/latest) 下载 `AndroidDevKit-0.1.0-setup.exe`，按当前用户安装；或解压 `AndroidDevKit-0.1.0-windows-x64.zip` 后运行 `AndroidDevKit.exe`。本机编译产物位于 `artifacts/windows/`。需要 Windows 11 x64 和 WebView2 Runtime；安装器在缺少 Runtime 时下载官方引导程序。当前发布包未签名，未进行干净 Windows 机器的安装 / 卸载验收。

首次打开可以直接查看现有 AVD 和设备。新 AVD 默认保存在 `%LOCALAPPDATA%\AndroidDevKit\avd`，另只读发现用户 `.android\avd`。SDK 下载、升级、删除及其他写操作先生成具体计划，确认作用范围后执行。使用 Google / Android 组件时需要查看并接受对应许可。

应用状态、任务数据库、下载缓存和截图位于 `%LOCALAPPDATA%\AndroidDevKit`。测试可通过 `ANDROID_DEVKIT_DATA_DIR` 隔离应用数据；该变量不改变所选 SDK。

## 已实现

界面支持简体中文 / English，可在标题栏快捷切换或在设置的“界面偏好”中选择；立即生效并自动保存，重启后恢复。切换保留表单输入、SDK 选择和在途任务，托盘菜单同步更新。SDK 包名、用户自定义名称、路径、原始日志与官方许可保留原文；任务状态、操作计划和应用错误提示按当前语言显示。

支持亮色 / 暗色主题，设置页与标题栏快捷按钮保持同步，立即生效并自动保存。主要页面、弹窗、表单、SDK 树、任务、日志和导出预览共用语义色变量；启动时先读取主题偏好，原生窗口和表单控件也同步切换。

- M0：Tauri 原生窗口桥接，Windows 11 最大化按钮悬停贴靠，DPI 感知；工具链兼容验证，参数数组进程执行和输出限流。
- M1：Rust 生成 TypeScript IPC 类型，SQLite 任务与审计记录，资源锁，SDK / AVD / USB / 无线会话发现；冻结设备目标安装单 APK、启动应用、截图预览 / 保存 / 复制和基础 Logcat。
- M2：用途最小化环境引导；官方仓库目录、许可、缺失 Java 私有运行时；SDK 组件安装 / 卸载、依赖最低版本检查、占用拦截、网络代理和逐项状态复查。
- M3：手机 / 平板 AVD 创建、配置编辑、就绪探测、打开窗口、冷启动、正常 / 强制停止、清数据和删除；无线配对、手动连接及断开。设备工作台另提供「网络 ADB」，按设备 IP 与端口直接连接已开启的 TCP/IP 调试服务（默认端口 5555，支持 IPv4 / IPv6），无需配对码；连接后验证实际可用性，可单独断开该端点。首次启用传统 TCP/IP 调试需先通过 USB 授权并执行 `adb -s <USB序列号> tcpip 5555`，应用不会自动开启设备端监听。配对成功与连接可用分开显示，配对码不写入任务数据库。
- M4 本轮：Logcat 级别 / Tag / 关键词 / PID / 应用过滤，应用进程集合刷新；暂停、清空游标；独立文件采集与配额 / 时限 / 轮转、缺口记录、崩溃后文件封口；默认脱敏、实际内容分页预览与范围选择；未保存产物退出保护、默认关闭的可选托盘；数据库 v1 → v2 备份迁移、全部在途任务恢复；NSIS 内部验收安装包与第三方许可清单。
- 任务中心：紧凑的两行任务摘要，详情、诊断与采集导出按需展开；按名称、设备 / 地址、结果或任务 ID 搜索全部历史，并与进行中、已完成、失败、待确认、待检查、已取消筛选组合。每页 30 项，进行中包含排队、运行和等待确认，许可确认始终直接可见。
- 待保存文件收进任务中心工具栏的文件保存图标，角标显示数量；点击弹窗查看全部文件的生成时间和大小，支持截图 / 文本预览、另存、逐项或全部“忽略并清理”。关闭预览后返回原列表并保留滚动位置，关闭按钮、Esc 和点击遮罩只关闭当前预览；预览中成功保存的文件在返回时移出待保存列表。忽略会删除对应截图 / 导出副本及导出元数据，并清除已失效的截图任务入口；已另存文件、原始日志采集段和任务历史保留。删除失败保留条目供重试；缺失文件可继续清理登记，不跟随链接或目录重定向。
- SDK 管理：SDK Platforms 按 API 展开平台和源码，系统镜像继续按 AOSP / Google APIs / Google Play → ABI → 具体组件分层；16 KB 等变体归入对应类型，未知厂商镜像单独保留。SDK Tools 按工具系列展开各版本。支持搜索、安装状态 / 更新 / 待变更筛选、过时与预览组件开关，跨分类和折叠保留变更选择；关键词与状态筛选展开匹配分支，“展开全部 / 收起全部”作用于全部层级。SDK Update Sites 展示四个官方源状态，并支持添加、编辑、停用和删除自定义 HTTPS 仓库（repository / remotePackage XML）；自定义组件安装使用应用私有 repositories.cfg，不修改 Android Studio 的源配置，重复组件 ID 的自定义源不加载。
- 模拟器配置：新建与编辑共用分组面板和屏幕预览，支持 CPU、内存、VM Heap、GPU、分辨率 / 密度 / 方向、前后摄像头、网络速度 / 延迟、快速 / 冷启动、内部容量、扩展存储、皮肤 / 边框、键盘、麦克风、电池和 GPS。仅提交改动字段，打开编辑后外部配置变化会阻止保存。内部容量修改不自动清数据，已有分区扩容由模拟器处理；新扩展存储由 mksdcard 创建，旧镜像保留，清数据会重置应用管理的镜像及覆盖层，外部镜像保留；检测到其他已登记 AVD 引用目标目录内的存储时阻止清理 / 删除。

每个会话的日志内存缓冲上限 10,000 行 / 8 MiB 原始文本，视图最多渲染 500 行。已停止且无消费者的会话仅缓存当前 ADB epoch 内最近访问的 4 个，上限合计 32 MiB 原始文本；失效会话在查看或采集结束后回收。仍在查看、采集或等待文件采集收尾的缓冲保留。应用过滤每 2 秒刷新包名主进程与冒号子进程；无法可靠归属的隔离进程不纳入。进程集合变化会记录缺口，刷新间隔可能遗漏新进程的初始日志。

文件采集每段最多 100 MiB，需选择总配额与时限；切页不停止采集，在任务中心停止并封口。断流、进程查询失败、用户停止或时限到达后固定结束序号和最后确认的进程集合，在配额内排空可归属的缓冲再封口；收尾不查询替换后的设备，查询失败记录缺口。导出可选择当前过滤范围或采集段，默认替换用户目录、设备标识、IP 和常见秘密字段；规则不能覆盖所有自由文本，保存前应检查实际预览。单次导出上限 64 MiB，超限显式截断，可分批选择采集段。

关闭时，无任务和未保存产物可直接退出；否则提供保存、忽略并清理对应缓存、等待 / 取消收尾。退出清理只作用于当前面板已展示的文件，新生成的文件仍需再次处理。托盘须在设置显式启用，启用后关闭窗口隐藏；托盘退出仍遵守检查。数据库首次升级到 v2 时，在原目录生成 `devkit.pre-v2-*.db` 备份，旧任务、设置保持可读。

设备写操作绑定 ADB Server epoch、transport ID 和 serial；断线后失败，不会改投其他设备。AVD 删除 / 重置绑定确认时的路径与配置，检查占用、共享数据和重解析点；共享系统镜像保留。中断任务先复查实际状态，不自动重放命令。

AVD 提交日志和索引均以临时文件同步落盘后原子提交，创建索引不覆盖已有文件。恢复时，能确认的自有截断索引会先备份为原目录中的 `*.ini.devkit-interrupted-*` 再修复；其他冲突保留原索引和提交日志供检查。未解决的提交记录阻止任务被复查为成功；复查同时验证索引 API 目标、路径和归属。启动恢复逐条处理；损坏的提交日志或采集清单隔离到应用数据目录的 `recovery` 子目录并保留诊断，单条错误不会阻止其余恢复。日志段已封口但清单尚未更新时，会补全暂存段的大小与摘要；已有封口摘要先校验，内容不符时保留原清单和文件供检查。

APK Manifest 输入最多 8 MiB，字符串解码与属性复制合计使用 16 MiB 内容预算；重复字符串偏移共享解析结果。任务关键状态保存失败会向界面反馈，执行前持久化失败不继续执行新步骤；取消请求写入失败不会报告取消成功。

工具进程的输入、执行、输出读取和最终退出等待共用一个截止时间；超时关闭本次调用持有的管道并结束直接子进程，写操作结果仍需复查。

默认窗口 1280 × 800，最小 480 × 640；小于 960 宽时使用紧凑导航和详情抽屉。此处根据 Windows Snap 实测将方案建议的 960 最小宽度调整为 480，以允许常见贴靠布局。

## 开发

本机已验证 Node.js 24.14.0、pnpm 11.25.0、Rust 1.95.0、MSVC / Windows SDK。依赖精确解析结果保存在 `pnpm-lock.yaml` 和 `src-tauri/Cargo.lock`，Rust 版本由 `rust-toolchain.toml` 固定。

```powershell
pnpm install --frozen-lockfile
pnpm tauri dev

# TypeScript + Vite + Rust Clippy（warnings 作为错误）
pnpm check
pnpm test

# 先运行 pnpm dev；隔离浏览器中的网络 ADB 表单回归，不访问真实设备
pnpm test:network-ui
pnpm test:tasks-ui
pnpm test:sdk-avd-ui

# 修改 Rust DTO 后重新生成前端类型
pnpm bindings

# Windows 可执行程序，不生成安装器
pnpm tauri build --no-bundle
# 输出 src-tauri/target/release/android-devkit.exe
powershell -NoProfile -File scripts/package-portable.ps1
# 输出 artifacts/windows/AndroidDevKit.exe 与 build-info.json

# 构建当前用户 NSIS 安装包，并生成锁定依赖许可清单
pnpm tauri build
# 输出 src-tauri/target/release/bundle/nsis/*-setup.exe
```

`pnpm dev` 使用 `http://127.0.0.1:19327` 仅预览界面；端口已被占用时明确报错，不自动切换。实际设备功能需要通过 `pnpm tauri dev` 或编译后的桌面程序运行。

## CI 与发布

[GitHub Actions](https://github.com/whoarei/android-devkit/actions/workflows/ci.yml) 在推送 `main` / `codex/**` 分支、Pull Request、`v*` 标签或手动触发时运行。Windows x64 流程使用固定版本的 Node.js、pnpm 和 Rust，安装锁定依赖，检查版本一致性、Rust 格式、TypeScript / Vite、Clippy、Rust 测试与生成的 IPC 类型；随后使用隔离 Chromium 测试网络 ADB、任务中心、SDK / AVD、中英文及主题界面，构建 NSIS 安装包并核对安装器内容。界面测试使用独立端口 `19328` 和模拟 IPC，不访问真实设备或 SDK。

成功后可在 Actions 中下载安装包、便携 ZIP、第三方许可、构建信息和 `SHA256SUMS.txt`；失败时保留界面截图。推送与应用版本一致的 `vX.Y.Z` 标签会在全部检查通过后自动创建正式 GitHub Release，发布同一批经过校验的构建产物。日常 CI 仅有读取权限，只有发布任务获得仓库内容写入权限；无需配置额外令牌。

后续发布先同步修改 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 中本项目的版本与 `src-tauri/tauri.conf.json`，界面版本从 `package.json` 读取。检查、提交并推送后创建对应标签：

```powershell
pnpm check:version
git tag -a v0.1.0 -m "Android DevKit v0.1.0"
git push origin main
git push origin v0.1.0
```

本地可用 `pnpm build && pnpm test:ui-fixtures` 运行相同的界面回归（默认使用 Edge；设置 `PLAYWRIGHT_CHANNEL=chromium` 时先运行 `pnpm exec playwright install chromium`）。完整 `pnpm tauri build` 后运行 `powershell -NoProfile -File scripts/package-release.ps1`，输出 `artifacts/release/`。发布版本从早期内部里程碑编号重新以 `0.1.0` 起算；更高内部版本的安装器会阻止降级，测试用户可继续使用便携包。

## 验证

默认 `pnpm test` 运行不下载组件、不操作用户 AVD 的测试，另有 7 个显式忽略项目：类型生成、4 类集成验收、SDK 目录只读联网验证与吞吐基准。下列命令按需执行：

```powershell
# 仅在 .qa/runtime 下创建一次性 AVD；使用现有 E:\Android\sdk
pnpm verify:m3

# 只读获取四个官方源，验证稳定与预览目录，不安装组件
cargo test --manifest-path src-tauri/Cargo.toml --lib sdk_repository_catalog -- --ignored --nocapture

# 在 M3 生命周期上追加真实应用 PID 重启、文件采集与脱敏导出
pnpm verify:m4
# 纯解析与环形缓冲吞吐，不等同于 UI / 磁盘全链路性能
pnpm verify:perf

# 在 .qa/runtime 下创建空 SDK，下载官方工具与私有 Java，不改 E:\Android\sdk
pnpm verify:m2

# 真实 adb 的失败路径及配对码持久化检查，只访问 localhost:9
pnpm verify:wireless

# 启动独立数据目录的本机 UI 验证实例，临时开放 CDP9224
powershell -NoProfile -File scripts/start-preview.ps1 -BuildProfile release
pnpm test:ui
node scripts/check-m4-ui.cjs
node scripts/check-live-log-ui.cjs
node scripts/check-compact.cjs
node scripts/window-dom.cjs
node scripts/stop-preview.cjs
# 仅对测试产生的未另存产物，显式确认放弃后结束 QA 实例
node scripts/stop-preview.cjs --discard
```

`scripts/check-window.py` 使用 Windows 原生 API 与 Pillow 检查命中区域并截图；贴靠浮层的截图需要人工视觉复核。测试截图及 JSON 证据位于 `.qa/app`，真实 SDK 测试记录位于 `.qa/runtime/m2-*` 和 `m3-*`。集成测试只删除自行创建的测试 AVD，保留共享镜像。

本机已通过空 SDK 引导、真实 AVD 生命周期、APK 安装启动和截图；无线配对失败 / 连接未就绪不会误报成功。**尚无手机参与成功配对验收**。多物理设备、跨显示器混合 DPI、全新普通 Windows 用户环境仍需补充硬件验收；模拟器验收不能代替这些证据。

## 待补充验收与发布

实体手机成功配对、USB 授权与双物理设备；跨显示器混合 DPI；干净普通 Windows 用户的引导 / 安装 / 卸载；磁盘满、网络中断、进程强杀的完整故障注入矩阵；长时间真实高吞吐 UI / 磁盘压力；签名安装包与发布更新。当前不声称已通过 PRD 全部 P0 验收。

随包的 `THIRD_PARTY_NOTICES.txt` 与 `third-party-manifest.json` 从锁定依赖生成，包含生产 npm、Windows Rust 普通 / 构建依赖和准确版本源码链接。此前 M3 的 0.3.0 可执行程序保留为 `artifacts/windows/AndroidDevKit-0.3.0.exe`。

## 设计依据

- [产品需求](docs/00000_20261017232447_Android-DevKit-PRD.html)
- [Tauri 技术方案](docs/00001_20261017234246_Android-DevKit-Technical-Design.html)
- [M3 实现与验证报告](docs/00002_20261018011008_Android-DevKit-M3-Implementation-Report.html)
- [M4 核心功能与内部验收报告](docs/00003_20261018063845_Android-DevKit-M4-Progress-Report.html)
- `src-tauri/resources/toolchain.json`：已验证版本组合；引导下载校验值见同目录 `bootstrap.json`，私有 Java 许可见 `java-license.txt`。

Windows 工具链当前冻结在已验证的 Command-line Tools 22.0；后续 Android CLI 迁移需另做兼容验收。参考 [Android sdkmanager](https://developer.android.com/tools/sdkmanager)、[模拟器命令行](https://developer.android.com/studio/run/emulator-commandline)、[Windows Snap 自定义标题栏要求](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-snap-layout-menu)。
