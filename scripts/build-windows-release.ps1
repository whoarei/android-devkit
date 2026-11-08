param(
    [ValidateRange(0, 60)]
    [int]$RetryDelaySeconds = 10
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

# Compile once. Retrying a transient bundler download must not rebuild Rust or
# run beforeBuildCommand again. Both phases retain the release signing config.
& pnpm tauri build --config src-tauri/tauri.release.conf.json --no-bundle -- --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$releaseExecutable = Join-Path $PWD 'src-tauri/target/release/android-devkit.exe'
$releaseUnbundled = Join-Path $PWD ('src-tauri/target/release/.unbundled-' + [guid]::NewGuid() + '.exe')
Copy-Item -LiteralPath $releaseExecutable -Destination $releaseUnbundled

try {
    for ($releaseBundleAttempt = 1; $releaseBundleAttempt -le 4; $releaseBundleAttempt++) {
        # Bundling patches the embedded bundle marker in place even when a later
        # download fails. Always start each attempt from the pristine executable.
        Copy-Item -LiteralPath $releaseUnbundled -Destination $releaseExecutable -Force
        $releaseBundleOutput = @(& pnpm tauri bundle --config src-tauri/tauri.release.conf.json --bundles nsis 2>&1)
        $releaseBundleExit = $LASTEXITCODE
        $releaseBundleOutput | ForEach-Object { Write-Host $_ }
        if ($releaseBundleExit -eq 0) { exit 0 }

        $releaseBundleNetworkFailure = ($releaseBundleOutput -join "`n") -match '(?i)http status:\s*(408|429|500|502|503|504)\b|connection reset|connection timed out|operation timed out|unexpected eof'
        if (-not $releaseBundleNetworkFailure -or $releaseBundleAttempt -eq 4) {
            exit $releaseBundleExit
        }
        Write-Warning "Temporary bundler download failure; retrying packaging ($releaseBundleAttempt/4) without recompiling."
        Start-Sleep -Seconds $RetryDelaySeconds
    }
} finally {
    Remove-Item -LiteralPath $releaseUnbundled -Force
}
