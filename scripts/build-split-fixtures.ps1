$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'build-test-apk.ps1')
if ($LASTEXITCODE -ne 0) { throw 'Base fixture build failed' }
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$fixtureRoot = Join-Path $repositoryRoot '.qa\runtime\fixture'
$splitRoot = Join-Path $fixtureRoot 'splits'
New-Item -ItemType Directory -Path $splitRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $fixtureRoot 'fixture.apk') -Destination (Join-Path $splitRoot 'base.apk') -Force
$javaRoot = 'C:\Program Files\Android\Android Studio\jbr\bin'
$toolsRoot = 'E:\Android\sdk\build-tools\35.0.0'
$androidJar = 'E:\Android\sdk\platforms\android-29\android.jar'
$keyFile = Join-Path $fixtureRoot 'qa.keystore'
$baseManifest = Get-Content -LiteralPath (Join-Path $fixtureRoot 'AndroidManifest.xml') -Raw
$baseVersion = [regex]::Match($baseManifest, 'android:versionCode="(\d+)"').Groups[1].Value
foreach ($fixture in @(
  @{ Name='config.zh'; Package='dev.androiddevkit.fixture'; Version=$baseVersion; Uses='' },
  @{ Name='feature.extra'; Package='dev.androiddevkit.fixture'; Version=$baseVersion; Uses='<uses-split android:name="feature.missing" />' },
  @{ Name='wrong.version'; Package='dev.androiddevkit.fixture'; Version='999'; Uses='' },
  @{ Name='wrong.package'; Package='dev.androiddevkit.other'; Version=$baseVersion; Uses='' }
)) {
  $manifest = '<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="'+$fixture.Package+'" split="'+$fixture.Name+'" android:versionCode="'+$fixture.Version+'"><uses-sdk android:minSdkVersion="23" android:targetSdkVersion="29" />'+$fixture.Uses+'<application android:hasCode="false" /></manifest>'
  $manifestPath = Join-Path $splitRoot 'AndroidManifest.xml'
  [System.IO.File]::WriteAllText($manifestPath, $manifest, [System.Text.UTF8Encoding]::new($false))
  $unsigned = Join-Path $splitRoot 'unsigned.apk'
  & (Join-Path $toolsRoot 'aapt.exe') package -f -M $manifestPath -I $androidJar -F $unsigned
  if ($LASTEXITCODE -ne 0) { throw "aapt split failed: $($fixture.Name)" }
  $output = Join-Path $splitRoot ($fixture.Name+'.apk')
  & (Join-Path $javaRoot 'java.exe') -jar (Join-Path $toolsRoot 'lib\apksigner.jar') sign --ks $keyFile --ks-pass pass:android --key-pass pass:android --out $output $unsigned
  if ($LASTEXITCODE -ne 0) { throw "APK signing failed: $($fixture.Name)" }
}
Write-Output $splitRoot
$otherKey = Join-Path $fixtureRoot 'other-qa.keystore'
if (-not (Test-Path -LiteralPath $otherKey)) {
  & (Join-Path $javaRoot 'keytool.exe') -genkeypair -keystore $otherKey -storepass android -keypass android -alias qa -keyalg RSA -keysize 2048 -validity 3650 -dname 'CN=Other DevKit QA'
  if ($LASTEXITCODE -ne 0) { throw 'Other QA key generation failed' }
}
& (Join-Path $javaRoot 'java.exe') -jar (Join-Path $toolsRoot 'lib\apksigner.jar') sign --ks $otherKey --ks-pass pass:android --key-pass pass:android --out (Join-Path $splitRoot 'other-signer.apk') (Join-Path $splitRoot 'base.apk')
if ($LASTEXITCODE -ne 0) { throw 'Other signer fixture failed' }
$validRoot = Join-Path $splitRoot 'valid'
$upgradeRoot = Join-Path $splitRoot 'v2'
$upgradeWork = Join-Path $fixtureRoot 'split-v2-work'
foreach ($part in @($validRoot,$upgradeRoot,$upgradeWork)) { New-Item -ItemType Directory -Path $part -Force | Out-Null }
foreach ($name in @('base.apk','config.zh.apk')) { Copy-Item -LiteralPath (Join-Path $splitRoot $name) -Destination (Join-Path $validRoot $name) -Force }
$upgradeManifest = $baseManifest.Replace('android:versionCode="1"','android:versionCode="2"').Replace('android:versionName="1.0"','android:versionName="2.0"')
$upgradeManifestPath = Join-Path $upgradeWork 'AndroidManifest.xml'
[System.IO.File]::WriteAllText($upgradeManifestPath, $upgradeManifest, [System.Text.UTF8Encoding]::new($false))
$upgradeUnsigned = Join-Path $upgradeWork 'base-unsigned.apk'
& (Join-Path $toolsRoot 'aapt.exe') package -f -M $upgradeManifestPath -I $androidJar -F $upgradeUnsigned
if ($LASTEXITCODE -ne 0) { throw 'Upgrade base aapt failed' }
$upgradeZip = [System.IO.Compression.ZipFile]::Open($upgradeUnsigned,[System.IO.Compression.ZipArchiveMode]::Update)
[System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($upgradeZip,(Join-Path $fixtureRoot 'dex\classes.dex'),'classes.dex') | Out-Null
$upgradeZip.Dispose()
& (Join-Path $javaRoot 'java.exe') -jar (Join-Path $toolsRoot 'lib\apksigner.jar') sign --ks $keyFile --ks-pass pass:android --key-pass pass:android --out (Join-Path $upgradeRoot 'base.apk') $upgradeUnsigned
if ($LASTEXITCODE -ne 0) { throw 'Upgrade base signing failed' }
$upgradeSplitManifest = '<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="dev.androiddevkit.fixture" split="config.zh" android:versionCode="2"><uses-sdk android:minSdkVersion="23" android:targetSdkVersion="29" /><application android:hasCode="false" /></manifest>'
[System.IO.File]::WriteAllText($upgradeManifestPath,$upgradeSplitManifest,[System.Text.UTF8Encoding]::new($false))
$upgradeSplitUnsigned = Join-Path $upgradeWork 'split-unsigned.apk'
& (Join-Path $toolsRoot 'aapt.exe') package -f -M $upgradeManifestPath -I $androidJar -F $upgradeSplitUnsigned
if ($LASTEXITCODE -ne 0) { throw 'Upgrade split aapt failed' }
& (Join-Path $javaRoot 'java.exe') -jar (Join-Path $toolsRoot 'lib\apksigner.jar') sign --ks $keyFile --ks-pass pass:android --key-pass pass:android --out (Join-Path $upgradeRoot 'config.zh.apk') $upgradeSplitUnsigned
if ($LASTEXITCODE -ne 0) { throw 'Upgrade split signing failed' }
& (Join-Path $javaRoot 'java.exe') -jar (Join-Path $toolsRoot 'lib\apksigner.jar') sign --ks $otherKey --ks-pass pass:android --key-pass pass:android --out (Join-Path $splitRoot 'other-signer-v2.apk') (Join-Path $upgradeRoot 'base.apk')
if ($LASTEXITCODE -ne 0) { throw 'Upgrade conflicting signer failed' }
