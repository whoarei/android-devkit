param([int]$DebugPort = 9224, [ValidateSet('debug', 'release')][string]$BuildProfile = 'debug')
$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$previewRoot = Join-Path $repositoryRoot '.qa\app'
New-Item -ItemType Directory -Path $previewRoot -Force | Out-Null
$executable = Join-Path $previewRoot 'AndroidDevKit-preview.exe'
Copy-Item -LiteralPath (Join-Path $repositoryRoot "src-tauri\target\$BuildProfile\android-devkit.exe") -Destination $executable
$previousBrowserArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$previousDataDirectory = $env:ANDROID_DEVKIT_DATA_DIR
try {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
  $env:ANDROID_DEVKIT_DATA_DIR = Join-Path $repositoryRoot '.qa\runtime\ui'
  $previewProcess = Start-Process -FilePath $executable -PassThru -WindowStyle Hidden
  Write-Output "Preview PID: $($previewProcess.Id); CDP port: $DebugPort"
} finally {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousBrowserArguments
  $env:ANDROID_DEVKIT_DATA_DIR = $previousDataDirectory
}
