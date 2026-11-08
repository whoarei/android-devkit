param(
    [Parameter(Mandatory=$true)][string]$BundletoolJar,
    [string]$SdkRoot = 'E:\Android\sdk',
    [string]$JavaBin = 'C:\Program Files\Android\Android Studio\jbr\bin',
    [string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$qaRoot = [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot '.qa\runtime'))
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $qaRoot 'fixture\containers' }
$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
if (-not $outputRoot.StartsWith($qaRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Container fixtures must remain beneath .qa/runtime'
}
$fixtureRoot = Join-Path $qaRoot 'fixture'
if (-not (Test-Path -LiteralPath (Join-Path $fixtureRoot 'dex\classes.dex'))) {
    & (Join-Path $PSScriptRoot 'build-split-fixtures.ps1')
    if ($LASTEXITCODE -ne 0) { throw 'Fixture build failed' }
}
$jar = (Resolve-Path -LiteralPath $BundletoolJar).Path
$toolsRoot = Join-Path $SdkRoot 'build-tools\35.0.0'
$androidJar = Join-Path $SdkRoot 'platforms\android-29\android.jar'
$work = Join-Path $outputRoot ('build-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work -Force | Out-Null
$proto = Join-Path $work 'proto.apk'
& (Join-Path $toolsRoot 'aapt2.exe') link --proto-format --manifest (Join-Path $fixtureRoot 'AndroidManifest.xml') -I $androidJar -o $proto
if ($LASTEXITCODE -ne 0) { throw 'aapt2 protobuf package build failed' }
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$module = Join-Path $work 'base.zip'
$inputZip = [System.IO.Compression.ZipFile]::OpenRead($proto)
$moduleZip = [System.IO.Compression.ZipFile]::Open($module,[System.IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($entry in $inputZip.Entries) {
        if ($entry.FullName.EndsWith('/')) { continue }
        $targetName = if ($entry.FullName -eq 'AndroidManifest.xml') { 'manifest/AndroidManifest.xml' } else { $entry.FullName }
        $targetEntry = $moduleZip.CreateEntry($targetName)
        $inputStream = $entry.Open()
        $outputStream = $targetEntry.Open()
        try { $inputStream.CopyTo($outputStream) } finally { $inputStream.Dispose(); $outputStream.Dispose() }
    }
    [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($moduleZip,(Join-Path $fixtureRoot 'dex\classes.dex'),'dex/classes.dex') | Out-Null
} finally { $inputZip.Dispose(); $moduleZip.Dispose() }
$bundle = Join-Path $work 'fixture.aab'
& (Join-Path $JavaBin 'java.exe') -jar $jar build-bundle "--modules=$module" "--output=$bundle"
if ($LASTEXITCODE -ne 0) { throw 'Official bundletool build-bundle failed' }
$apks = Join-Path $work 'official.apks'
& (Join-Path $JavaBin 'java.exe') -jar $jar build-apks "--bundle=$bundle" "--output=$apks" "--aapt2=$(Join-Path $toolsRoot 'aapt2.exe')" "--ks=$(Join-Path $fixtureRoot 'qa.keystore')" --ks-pass=pass:android --ks-key-alias=qa --key-pass=pass:android
if ($LASTEXITCODE -ne 0) { throw 'Official bundletool build-apks failed' }
$destination = Join-Path $outputRoot 'official.apks'
Copy-Item -LiteralPath $apks -Destination $destination -Force
Write-Output $destination
