param([switch]$WithUpdater)
$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $repositoryRoot 'package.json') -Raw | ConvertFrom-Json).version
& (Join-Path $PSScriptRoot 'package-portable.ps1') -BuildProfile release
$portableDirectory = Join-Path $repositoryRoot 'artifacts/windows'
$outputDirectory = Join-Path $repositoryRoot 'artifacts/release'
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
$installerName = "AndroidDevKit-$version-setup.exe"
$installerPath = Join-Path $portableDirectory $installerName
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) { throw 'Build the NSIS installer before packaging a release.' }
$portableFiles = @('AndroidDevKit.exe', 'THIRD_PARTY_NOTICES.txt', 'third-party-manifest.json', 'build-info.json')
$portablePaths = $portableFiles | ForEach-Object { Join-Path $portableDirectory $_ }
$archiveName = "AndroidDevKit-$version-windows-x64.zip"
Compress-Archive -LiteralPath $portablePaths -DestinationPath (Join-Path $outputDirectory $archiveName) -Force
$releaseFiles = @($archiveName, $installerName, 'THIRD_PARTY_NOTICES.txt', 'third-party-manifest.json', 'build-info.json')
foreach ($file in $releaseFiles | Where-Object { $_ -ne $archiveName }) {
  Copy-Item -LiteralPath (Join-Path $portableDirectory $file) -Destination (Join-Path $outputDirectory $file)
}
if ($WithUpdater) {
  $bundledInstaller = Get-ChildItem -LiteralPath (Join-Path $repositoryRoot 'src-tauri/target/release/bundle/nsis') -Filter "*$version*setup.exe" | Select-Object -First 1
  if (-not $bundledInstaller) { throw 'Missing signed NSIS installer.' }
  $signaturePath = "$($bundledInstaller.FullName).sig"
  if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) { throw 'Missing updater signature; build with tauri.release.conf.json and the signing key.' }
  if ((Get-FileHash -LiteralPath $bundledInstaller.FullName -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash) { throw 'Signed installer and packaged installer differ.' }
  Copy-Item -LiteralPath $signaturePath -Destination (Join-Path $outputDirectory "$installerName.sig")
  node (Join-Path $PSScriptRoot 'create-updater-manifest.cjs') $outputDirectory $version
  if ($LASTEXITCODE -ne 0) { throw 'Could not generate the updater manifest.' }
  $releaseFiles += @("$installerName.sig", 'latest.json')
}
$checksums = foreach ($file in $releaseFiles) {
  $digest = (Get-FileHash -LiteralPath (Join-Path $outputDirectory $file) -Algorithm SHA256).Hash.ToLowerInvariant()
  "$digest  $file"
}
[IO.File]::WriteAllText((Join-Path $outputDirectory 'SHA256SUMS.txt'), ($checksums -join "`n") + "`n", [Text.Encoding]::ASCII)
Write-Output "Release $version packaged in $outputDirectory"
