# Android DevKit 当前发布约定

本文件记录创建 skill 时核对过的项目约定。每次使用时与仓库源码比较，变更后的仓库优先；不要复制历史发布号或机器配置。

## 身份、版本和说明

- 项目：`package.json` 的 `name` 为 `android-devkit`，Tauri identifier 为 `dev.android-devkit.desktop`。
- 当前已配置的公共仓库：`whoarei/android-devkit`。每次先核对 `git remote get-url origin`，从实际远端确定 `gh --repo`，不把这个名称用于其他项目。
- 发布分支目前为 `main`；`main` / `codex/**` 推送、PR 和手动运行均执行检查，只有 `push` 到 `v*` 标签才执行 Release 发布。
- `scripts/check-version.cjs` 核对四个版本文件；传入标签可额外核对版本匹配。它目前只接受稳定的三段数字版本。
- `pnpm-lock.yaml` 当前不保存根项目版本；不要因改版本而重解全部依赖。Cargo.lock 只更新根项目条目，并检查差异。
- Release 正文目前内嵌在 `.github/workflows/ci.yml` 的 `Publish verified assets` 步骤。后续发版应把正文更新为从上次发布到这次候选提交的实际变更，保留仍适用的安装要求和限制，不沿用初版功能清单冒充更新日志。
- README 当前版本、下载文件名必须同步；命令历史示例是否保留按上下文判断，不能全局替换旧版本文本。
- 项目正式新文档遵守 AGENTS 的五位序号和北京时间命名规则；修改已有正式文档保留文件名。临时发布说明可用临时文件，传给 `--notes-file`，不需要创建额外交付文档。

## 发布前检查

Windows PowerShell 中用 `pnpm.cmd` 避开脚本 shim 限制。逐条执行并检查退出码，不能在长串命令中忽略前一步失败。工具链版本从 `rust-toolchain.toml`、`package.json` 和 CI 读取，不在 skill 内锁定副本。

```powershell
git status --short
git remote get-url origin
git log --oneline origin/main..HEAD
gh auth status
gh release list --repo $releaseRepo
git ls-remote --tags origin

$releaseVersion = (Get-Content -LiteralPath package.json -Raw | ConvertFrom-Json).version
$releaseTag = "v$releaseVersion"
node scripts/check-version.cjs $releaseTag

pnpm.cmd install --frozen-lockfile
cargo fetch --locked --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
pnpm.cmd check
cargo test --locked --manifest-path src-tauri/Cargo.toml
pnpm.cmd bindings
git diff --exit-code -- src/bindings.ts
pnpm.cmd exec playwright install chromium
$env:PLAYWRIGHT_CHANNEL = 'chromium'
pnpm.cmd test:ui-fixtures
```

`$releaseRepo` 必须先从核对后的远端确定。`origin/main` 须先正常 fetch 更新后才可用于判断未推送提交。浏览器 fixture 使用独立端口 `19328` 和模拟 IPC，不访问真实设备；不要停止用户正在运行的开发服务来腾端口。IPC 生成差异必须处理并提交，不通过撤销生成结果来让检查变绿。

## 构建、资产和核验

```powershell
pnpm.cmd tauri build -- --locked
& './scripts/package-release.ps1'
node scripts/check-installer.cjs 'C:/Program Files/7-Zip/7z.exe'
```

当前产物关系：

| 路径 | 内容 |
| --- | --- |
| `src-tauri/target/release/android-devkit.exe` | 原始 release 程序 |
| `src-tauri/target/release/bundle/nsis/*-setup.exe` | NSIS 安装包 |
| `artifacts/windows/` | 本地易运行程序、安装包、许可和构建信息 |
| `artifacts/release/` | CI 上传与 Release 使用的发行文件 |

`artifacts/release/` 只应包含：

1. `AndroidDevKit-X.Y.Z-setup.exe`
2. `AndroidDevKit-X.Y.Z-windows-x64.zip`
3. `THIRD_PARTY_NOTICES.txt`
4. `third-party-manifest.json`
5. `build-info.json`
6. `SHA256SUMS.txt`

ZIP 内恰好为 `AndroidDevKit.exe`、两份许可文件和 `build-info.json`。`SHA256SUMS.txt` 对除自身之外的五个 Release 文件求摘要；build-info 另外记录 ZIP 内原始 EXE 的大小和摘要，以及安装器摘要。校验器同时核对这些关系。

打包脚本会保留输出目录旧文件，并且可能复制同版本的旧安装器，所以必须先成功完成本次 NSIS 构建，不能拿历史 `artifacts/` 目录证明候选提交已打包。CI 的干净检出可避免此问题。`scripts/check-installer.cjs` 解包检查安装器与便携程序的 payload；Tauri 会将内嵌 bundle marker 从 `UNK` 改成 `NSS`，归一化后应一致。不要用安装器内 EXE 的原始摘要直接要求等于便携 EXE 摘要。

目前安装模式为当前用户，发行平台 Windows x64；需要 Windows 11 与 WebView2。安装器缺少 WebView2 时使用官方引导程序。签名状态、干净机器安装 / 卸载及其他限制以这次实际检查为准，不因打包成功就宣称验证完成。

## 远端发布和跟踪

发布源代码与计划确认后记录 `$releaseCommit`。下列远端写入只在当前请求已经授权正式发布时执行：

```powershell
$releaseCommit = git rev-parse HEAD
git push origin main
git tag -a $releaseTag $releaseCommit -m "Android DevKit $releaseTag"
git push origin "refs/tags/$releaseTag"

gh run list --repo $releaseRepo --workflow ci.yml --event push --commit $releaseCommit --json databaseId,headBranch,headSha,status,conclusion,url
gh run view $releaseRunId --repo $releaseRepo --json headSha,headBranch,event,status,conclusion,jobs,url
gh release view $releaseTag --repo $releaseRepo --json tagName,isDraft,isPrerelease,url,assets,body
git ls-remote origin "refs/tags/$releaseTag" "refs/tags/$releaseTag^{}"
```

不要未经核对直接运行这段：先确认当前分支与 `main`、候选 SHA 的关系，以及标签未被其他提交占用。`$releaseRunId` 应取 `headBranch` 等于目标标签、`headSha` 等于候选 SHA 的标签 push 运行；有多个重跑时核对 attempt。注解标签比较解引用的 `^{}` SHA，不能把 tag object 的 SHA 当作代码提交。

当前 CI 的 `windows` job 成功后，`release` job 下载同一 SHA 的 `windows-x64-*` 资产、检查 SHA256SUMS，再通过 `gh release create --verify-tag --latest` 发布。只有标签工作流含发布 job；不以普通 main 构建代替。无需额外调用创建 Release 命令。

下载核验时在系统临时目录创建两个全新目录，使用确定的版本和资产名：

```powershell
$downloadDirectory = Join-Path ([IO.Path]::GetTempPath()) ("android-devkit-release-" + [guid]::NewGuid())
$portableDirectory = Join-Path ([IO.Path]::GetTempPath()) ("android-devkit-portable-" + [guid]::NewGuid())
gh release download $releaseTag --repo $releaseRepo --dir $downloadDirectory
Expand-Archive -LiteralPath (Join-Path $downloadDirectory "AndroidDevKit-$releaseVersion-windows-x64.zip") -DestinationPath $portableDirectory
node $assetVerifier $downloadDirectory $releaseVersion --portable $portableDirectory
```

`$assetVerifier` 指向本 skill 的 `scripts/verify-release-assets.cjs`。该脚本不访问网络、不上传、不删除文件。临时目录使用完按需要清理，先核对绝对路径后再删除。

若 GitHub CLI 连接失败，先核对认证与已有会话中的代理设置；只在当前进程使用已确认的代理，不写死历史端口、不改全局 Git / 系统代理，不输出令牌。认证或网络问题报告为阻塞原因，不能据此新建仓库、重复发布或忽略远端检查。
