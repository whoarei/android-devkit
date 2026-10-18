param([ValidateSet('debug', 'release')][string]$BuildProfile = 'release')
$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$sourceExecutable = Join-Path $repositoryRoot "src-tauri\target\$BuildProfile\android-devkit.exe"
if (-not (Test-Path -LiteralPath $sourceExecutable -PathType Leaf)) { throw '请先运行 pnpm tauri build --no-bundle' }
$outputDirectory = Join-Path $repositoryRoot 'artifacts\windows'
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
$destinationExecutable = Join-Path $outputDirectory 'AndroidDevKit.exe'
Copy-Item -LiteralPath $sourceExecutable -Destination $destinationExecutable
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'src-tauri/resources/THIRD_PARTY_NOTICES.txt') -Destination (Join-Path $outputDirectory 'THIRD_PARTY_NOTICES.txt')
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'src-tauri/resources/third-party-manifest.json') -Destination (Join-Path $outputDirectory 'third-party-manifest.json')
$packageMetadata = Get-Content -LiteralPath (Join-Path $repositoryRoot 'package.json') -Raw | ConvertFrom-Json
$created = [TimeZoneInfo]::ConvertTimeBySystemTimeZoneId([DateTimeOffset]::UtcNow, 'China Standard Time')
$information = [ordered]@{
  product = 'Android DevKit'
  version = $packageMetadata.version
  profile = $BuildProfile
  platform = 'windows-x86_64'
  createdBeijing = $created.ToString('yyyy-MM-dd HH:mm:ss zzz')
  sha256 = (Get-FileHash -LiteralPath $destinationExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
  sizeBytes = (Get-Item -LiteralPath $destinationExecutable).Length
  defaultSdk = 'E:\Android\sdk'
  signed = $false
}
$installer = Get-ChildItem -LiteralPath (Join-Path $repositoryRoot 'src-tauri/target/release/bundle/nsis') -Filter "*$($packageMetadata.version)*setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($installer) {
  $installerName = "AndroidDevKit-$($packageMetadata.version)-setup.exe"
  Copy-Item -LiteralPath $installer.FullName -Destination (Join-Path $outputDirectory $installerName)
  $information.installer = [ordered]@{ file = $installerName; sha256 = (Get-FileHash -LiteralPath (Join-Path $outputDirectory $installerName) -Algorithm SHA256).Hash.ToLowerInvariant(); signed = $false }
}
$information | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputDirectory 'build-info.json') -Encoding utf8
Write-Output $destinationExecutable
Write-Output "SHA256: $($information.sha256)"
