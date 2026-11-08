param([string]$ToolsDirectory = (Join-Path $env:LOCALAPPDATA 'tauri'))
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

# Match tauri-cli-v2.12.3/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs.
# Fetch from the original publishers through GitHub's asset API, avoiding the
# release-page download route that can return 504. Keep both upstream SHA-1
# pins and independent SHA-256 pins; never accept an unverified tool cache.
$releaseCliVersion = (Get-Content (Join-Path $PSScriptRoot '../package.json') -Raw | ConvertFrom-Json).devDependencies.'@tauri-apps/cli'
if ($releaseCliVersion -ne '2.12.3') { throw 'Review NSIS tool versions and hashes when changing the pinned Tauri CLI.' }
$releaseAssets = @(
    @{ Repo='tauri-apps/binary-releases'; Tag='nsis-3.13'; Name='nsis-3.13.zip'; Sha1='DB14F8AF2EA346B786C6AE19343F61AAF5E09B8E'; Sha256='ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8' },
    @{ Repo='tauri-apps/nsis-tauri-utils'; Tag='nsis_tauri_utils-v0.5.3'; Name='nsis_tauri_utils.dll'; Sha1='75197FEE3C6A814FE035788D1C34EAD39349B860'; Sha256='5ba143b5db4a87d32d6e7802e033330aae56cbceabe0d1e3ba41948385ad4709' }
)
$releaseStage = Join-Path ([IO.Path]::GetTempPath()) ('devkit-nsis-tools-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $releaseStage | Out-Null
foreach ($releaseAsset in $releaseAssets) {
    for ($releaseAttempt = 1; $releaseAttempt -le 4; $releaseAttempt++) {
        $releaseAttemptDirectory = Join-Path $releaseStage ($releaseAsset.Name + '-' + $releaseAttempt)
        New-Item -ItemType Directory -Path $releaseAttemptDirectory | Out-Null
        & gh release download $releaseAsset.Tag --repo $releaseAsset.Repo --pattern $releaseAsset.Name --dir $releaseAttemptDirectory
        if ($LASTEXITCODE -eq 0) { break }
        if ($releaseAttempt -eq 4) { throw "Could not download $($releaseAsset.Name) from its official GitHub release." }
        Start-Sleep -Seconds 10
    }
    $releaseFile = Join-Path $releaseAttemptDirectory $releaseAsset.Name
    if ((Get-FileHash -LiteralPath $releaseFile -Algorithm SHA1).Hash -ine $releaseAsset.Sha1 -or
        (Get-FileHash -LiteralPath $releaseFile -Algorithm SHA256).Hash -ine $releaseAsset.Sha256) {
        throw "NSIS tool hash mismatch: $($releaseAsset.Name)"
    }
    $releaseAsset.File = $releaseFile
}
$releaseExpanded = Join-Path $releaseStage 'expanded'
Expand-Archive -LiteralPath $releaseAssets[0].File -DestinationPath $releaseExpanded
$releaseSource = Join-Path $releaseExpanded 'nsis-3.13'
$releasePluginDirectory = Join-Path $releaseSource 'Plugins/x86-unicode/additional'
New-Item -ItemType Directory -Path $releasePluginDirectory -Force | Out-Null
Copy-Item -LiteralPath $releaseAssets[1].File -Destination (Join-Path $releasePluginDirectory 'nsis_tauri_utils.dll')
if ((Get-FileHash -LiteralPath (Join-Path $releaseSource 'NSIS.exe') -Algorithm SHA1).Hash -ine '6D45D4E8CCF18FE1FDAC9164058F356102A2ECF2') {
    throw 'Extracted NSIS executable does not match the pinned Tauri toolchain.'
}
$releaseRequiredFiles = @('makensis.exe','Bin/makensis.exe','Stubs/lzma-x86-unicode','Stubs/lzma_solid-x86-unicode','Include/MUI2.nsh','Include/FileFunc.nsh','Include/x64.nsh','Include/nsDialogs.nsh','Include/WinMessages.nsh','Include/Win/COM.nsh','Include/Win/Propkey.nsh','Include/Win/RestartManager.nsh')
foreach ($releaseRequiredFile in $releaseRequiredFiles) {
    if (-not (Test-Path -LiteralPath (Join-Path $releaseSource $releaseRequiredFile) -PathType Leaf)) { throw "Incomplete NSIS archive: $releaseRequiredFile" }
}
$releaseToolsRoot = [IO.Path]::GetFullPath($ToolsDirectory)
New-Item -ItemType Directory -Path $releaseToolsRoot -Force | Out-Null
$releaseDestination = [IO.Path]::GetFullPath((Join-Path $releaseToolsRoot 'NSIS'))
if (-not $releaseDestination.StartsWith($releaseToolsRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'NSIS destination escaped the selected tool directory.' }
if (Test-Path -LiteralPath $releaseDestination) {
    if ((Get-Item -LiteralPath $releaseDestination -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Refusing to replace a redirected NSIS cache.' }
    Remove-Item -LiteralPath $releaseDestination -Recurse -Force
}
# Copy only the verified toolchain into Tauri's dedicated NSIS cache.
Copy-Item -LiteralPath $releaseSource -Destination $releaseDestination -Recurse
Write-Output 'Prepared official NSIS 3.13 and tauri-utils 0.5.3; all pinned hashes verified.'
