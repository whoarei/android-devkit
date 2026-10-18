$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$fixtureRoot = Join-Path $repositoryRoot '.qa\runtime\fixture'
$javaRoot = 'C:\Program Files\Android\Android Studio\jbr\bin'
$sdkRoot = 'E:\Android\sdk'
$toolsRoot = Join-Path $sdkRoot 'build-tools\35.0.0'
$androidJar = Join-Path $sdkRoot 'platforms\android-29\android.jar'
foreach ($part in @('src','classes','dex')) { New-Item -ItemType Directory -Path (Join-Path $fixtureRoot $part) -Force | Out-Null }
$activitySource = @'
package dev.androiddevkit.fixture;
public class MainActivity extends android.app.Activity implements java.lang.Runnable {
  private android.os.Handler handler;
  public void onCreate(android.os.Bundle state) {
    super.onCreate(state);
    android.widget.TextView view = new android.widget.TextView(this);
    view.setText("Android DevKit M4 verification");
    view.setTextSize(24); view.setPadding(24,80,24,24);
    setContentView(view);
    android.util.Log.i("DevKitFixture", "M3 fixture started");
    handler = new android.os.Handler(android.os.Looper.getMainLooper());
    handler.post(this);
  }
  public void run() { android.util.Log.i("DevKitFixture", "M4 heartbeat token=fixture-secret 192.168.10.20"); handler.postDelayed(this, 500); }
  public void onDestroy() { if (handler != null) handler.removeCallbacks(this); super.onDestroy(); }
}
'@
[IO.File]::WriteAllText((Join-Path $fixtureRoot 'src\MainActivity.java'), $activitySource, (New-Object Text.UTF8Encoding $false))
$manifestSource = @'
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="dev.androiddevkit.fixture" android:versionCode="1" android:versionName="1.0"><uses-sdk android:minSdkVersion="23" android:targetSdkVersion="29"/><application android:label="DevKit M3 QA" android:theme="@android:style/Theme.Material.Light.NoActionBar"><activity android:name=".MainActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>
'@
[IO.File]::WriteAllText((Join-Path $fixtureRoot 'AndroidManifest.xml'), $manifestSource, (New-Object Text.UTF8Encoding $false))
& (Join-Path $javaRoot 'javac.exe') --release 8 -classpath $androidJar -d (Join-Path $fixtureRoot 'classes') (Join-Path $fixtureRoot 'src\MainActivity.java')
if ($LASTEXITCODE -ne 0) { throw 'javac failed' }
& (Join-Path $javaRoot 'java.exe') -cp (Join-Path $toolsRoot 'lib\d8.jar') com.android.tools.r8.D8 --lib $androidJar --min-api 23 --output (Join-Path $fixtureRoot 'dex') (Join-Path $fixtureRoot 'classes\dev\androiddevkit\fixture\MainActivity.class')
if ($LASTEXITCODE -ne 0) { throw 'D8 failed' }
& (Join-Path $toolsRoot 'aapt.exe') package -f -M (Join-Path $fixtureRoot 'AndroidManifest.xml') -I $androidJar -F (Join-Path $fixtureRoot 'unsigned.apk')
if ($LASTEXITCODE -ne 0) { throw 'aapt failed' }
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zipFile = [System.IO.Compression.ZipFile]::Open((Join-Path $fixtureRoot 'unsigned.apk'),[System.IO.Compression.ZipArchiveMode]::Update)
[System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zipFile,(Join-Path $fixtureRoot 'dex\classes.dex'),'classes.dex') | Out-Null
$zipFile.Dispose()
$keyFile = Join-Path $fixtureRoot 'qa.keystore'
if (-not (Test-Path -LiteralPath $keyFile)) {
  & (Join-Path $javaRoot 'keytool.exe') -genkeypair -keystore $keyFile -storepass android -keypass android -alias qa -keyalg RSA -keysize 2048 -validity 3650 -dname 'CN=DevKit QA'
}
& (Join-Path $javaRoot 'java.exe') -jar (Join-Path $toolsRoot 'lib\apksigner.jar') sign --ks $keyFile --ks-pass pass:android --key-pass pass:android --out (Join-Path $fixtureRoot 'fixture.apk') (Join-Path $fixtureRoot 'unsigned.apk')
if ($LASTEXITCODE -ne 0) { throw 'APK signing failed' }
Write-Output (Join-Path $fixtureRoot 'fixture.apk')
