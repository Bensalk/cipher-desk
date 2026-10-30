<#
Cipher Desk: set up a Windows PC and build the app on it.

  powershell -ExecutionPolicy Bypass -File .\build-on-this-pc.ps1 -KeyFolder "$HOME\Downloads"

What it does (safe to run again: finished steps are skipped):
  1. Installs the tools with winget: Git, Java 17, Rust, Visual Studio 2022 C++ build tools.
  2. Downloads Flutter and the Android SDK command-line tools into C:\cd\tools.
  3. Downloads the Cipher Desk code into C:\cd\cipher-desk (asks you to sign in to GitHub once).
  4. Builds the Android APK signed with your key, then the Windows app.
  5. Puts the results in a "Cipher Desk build" folder on your Desktop.

-KeyFolder: the folder with cipher-desk-release.jks and Cipher-Desk-Android-signing-SECRETS.txt.
The passwords are read from that file and written only to C:\cd\cipher-desk\app\android\key.properties
(git ignores it). Nothing secret is printed or sent anywhere.
#>
param(
    [string]$KeyFolder = "$HOME\Downloads",
    [string]$Branch = "repo-style",
    [switch]$SkipWindows
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue" # much faster downloads in Windows PowerShell
$Root = "C:\cd"
$Tools = "$Root\tools"
$Repo = "$Root\cipher-desk"
$Out = Join-Path ([Environment]::GetFolderPath("Desktop")) "Cipher Desk build"
New-Item -ItemType Directory -Force -Path $Root, $Tools, $Out | Out-Null
Start-Transcript -Path "$Out\build-log.txt" -Append | Out-Null

function Step($text) { Write-Host ""; Write-Host "==== $text ====" -ForegroundColor Cyan }
function Have($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
function RefreshPath {
    $env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
                [Environment]::GetEnvironmentVariable("Path", "User")
}
function Install-Tool($id, [string[]]$extra = @()) {
    Write-Host "Installing $id (Windows may ask for permission)..."
    winget.exe install --id $id -e --accept-source-agreements --accept-package-agreements @extra
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) {
        # -1978335189 = already installed
        throw "winget could not install $id (exit $LASTEXITCODE)"
    }
}
function Check($what) { if ($LASTEXITCODE -ne 0) { throw "$what failed (exit $LASTEXITCODE). See $Out\build-log.txt" } }

# ---------------------------------------------------------------- 0. checks
Step "Checking this PC"
$free = (Get-PSDrive C).Free / 1GB
Write-Host ("Free space on C: {0:N0} GB" -f $free)
if ($free -lt 25) { throw "At least 25 GB free space on C: is needed." }
if (-not (Have winget.exe)) { throw "winget is missing. Install 'App Installer' from the Microsoft Store, then run this again." }
# Flutter needs Windows "Developer Mode" (symbolic links) to build apps with plugins.
$devMode = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock" -ErrorAction SilentlyContinue
if (-not $devMode -or $devMode.AllowDevelopmentWithoutDevLicense -ne 1) {
    Write-Host "Please turn on 'Developer Mode' in the Settings window that opens, then come back here." -ForegroundColor Yellow
    Start-Process "ms-settings:developers"
    Read-Host "Press Enter after turning it on"
}
$jks = Join-Path $KeyFolder "cipher-desk-release.jks"
$secrets = Join-Path $KeyFolder "Cipher-Desk-Android-signing-SECRETS.txt"
if (-not (Test-Path $jks) -or -not (Test-Path $secrets)) {
    throw "Key files not found in $KeyFolder. Use -KeyFolder `"<folder with both files>`"."
}

# ---------------------------------------------------------------- 1. tools
Step "1/5 Installing tools (the longest part; Visual Studio tools take 20-40 minutes)"
if (-not (Have git)) { Install-Tool "Git.Git" }
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasVc = (Test-Path $vsWhere) -and (& $vsWhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if (-not $hasVc) {
    Install-Tool "Microsoft.VisualStudio.2022.BuildTools" @("--override",
        "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended")
}
if (-not (Get-ChildItem "$env:ProgramFiles\Microsoft\jdk-17*" -ErrorAction SilentlyContinue)) { Install-Tool "Microsoft.OpenJDK.17" }
if (-not (Have rustup) -and -not (Test-Path "$HOME\.cargo\bin\rustup.exe")) { Install-Tool "Rustlang.Rustup" }
RefreshPath
$env:Path = "$HOME\.cargo\bin;$env:Path"
$env:JAVA_HOME = (Get-ChildItem "$env:ProgramFiles\Microsoft\jdk-17*" | Select-Object -First 1).FullName
$env:Path = "$env:JAVA_HOME\bin;$env:Path"
git config --global core.longpaths true

Write-Host "Rust toolchain and Android targets..."
# The build tool (Cargokit) uses the "stable" toolchain.
# --no-self-update: a rustup update in the middle of the run confuses the next command.
rustup toolchain install stable --profile minimal --no-self-update; Check "rustup"
foreach ($try in 1..2) {
    rustup target add --toolchain stable aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
    if ($LASTEXITCODE -eq 0) { break }
    Start-Sleep -Seconds 3
}
Check "rustup targets"

# ---------------------------------------------------------------- 2. Flutter + Android SDK
Step "2/5 Flutter and the Android SDK"
$flutter = "$Tools\flutter"
if (-not (Test-Path "$flutter\bin\flutter.bat")) {
    git clone --depth 1 -b stable https://github.com/flutter/flutter.git $flutter; Check "Flutter download"
}
$env:Path = "$flutter\bin;$env:Path"
$sdk = "$Tools\android-sdk"
$sdkmanager = "$sdk\cmdline-tools\latest\bin\sdkmanager.bat"
if (-not (Test-Path $sdkmanager)) {
    $zip = "$Tools\cmdline-tools.zip"
    $url = "https://dl.google.com/android/repository/commandlinetools-win-15859902_latest.zip"
    Write-Host "Downloading the Android command-line tools..."
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($hash -ne "90ae805d20434428bffcb699c290860f19bb5f66a67e6b330067e3de801fb04a") {
        throw "The Android tools download is damaged (checksum mismatch)."
    }
    Expand-Archive $zip -DestinationPath "$Tools\ct" -Force
    New-Item -ItemType Directory -Force "$sdk\cmdline-tools" | Out-Null
    Move-Item "$Tools\ct\cmdline-tools" "$sdk\cmdline-tools\latest"
    Remove-Item $zip, "$Tools\ct" -Recurse -Force
}
$env:ANDROID_HOME = $sdk
$env:ANDROID_SDK_ROOT = $sdk
Write-Host "Accepting the Android SDK licences and installing the SDK parts..."
1..30 | ForEach-Object { "y" } | & $sdkmanager --licenses | Out-Null
& $sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0"; Check "Android SDK"
flutter config --android-sdk $sdk --jdk-dir $env:JAVA_HOME --no-analytics | Out-Null
flutter --disable-analytics | Out-Null
flutter precache --android --windows; Check "Flutter download of build files"

# ---------------------------------------------------------------- 3. code
Step "3/5 Downloading the Cipher Desk code (sign in to GitHub if a window opens)"
if (-not (Test-Path "$Repo\.git")) {
    git clone -b $Branch https://github.com/Bensalk/cipher-desk.git $Repo; Check "Code download"
} else {
    git -C $Repo fetch origin $Branch; Check "Code update"
    git -C $Repo checkout $Branch; Check "git checkout"
    git -C $Repo reset --hard "origin/$Branch"; Check "git reset"
}

# ---------------------------------------------------------------- 4. Android
Step "4/5 Building the Android app (15-25 minutes the first time)"
$text = Get-Content $secrets -Raw
function SecretValue($name) {
    $m = [regex]::Match($text, "Name:\s*$name\s*\r?\nValue:\s*(\S+)")
    if (-not $m.Success) { throw "$name not found in the secrets file." }
    $m.Groups[1].Value
}
$storePass = SecretValue "ANDROID_KEYSTORE_PASSWORD"
$keyPass = SecretValue "ANDROID_KEY_PASSWORD"
$alias = SecretValue "ANDROID_KEY_ALIAS"
$keyProps = "$Repo\app\android\key.properties"
@(
    "storeFile=$($jks -replace '\\','/')",
    "storePassword=$storePass",
    "keyAlias=$alias",
    "keyPassword=$keyPass"
) | Set-Content -Path $keyProps -Encoding ascii
Remove-Variable storePass, keyPass

Push-Location "$Repo\app"
try {
    flutter pub get; Check "flutter pub get"
    flutter build apk --release; Check "Android build"
    $version = (Select-String -Path pubspec.yaml -Pattern '^version:\s*([0-9.]+)').Matches[0].Groups[1].Value
    $apk = "$Out\CipherDesk-$version-android.apk"
    Copy-Item build\app\outputs\flutter-apk\app-release.apk $apk -Force
    Write-Host "APK ready: $apk" -ForegroundColor Green

    # Check it is signed with your key, not the debug key.
    $apksigner = Get-ChildItem "$sdk\build-tools\*\apksigner.bat" | Sort-Object FullName | Select-Object -Last 1
    & $apksigner.FullName verify --print-certs $apk | Tee-Object "$Out\android-signing-certificate.txt"
    if (Select-String -Path "$Out\android-signing-certificate.txt" -Pattern "Android Debug" -Quiet) {
        throw "The APK is signed with the debug key; check the key files."
    }

    # ------------------------------------------------------------ 5. Windows
    if (-not $SkipWindows) {
        Step "5/5 Building the Windows app (10-15 minutes)"
        flutter build windows --release; Check "Windows build"
        $zip = "$Out\CipherDesk-$version-windows-x64.zip"
        if (Test-Path $zip) { Remove-Item $zip }
        Compress-Archive -Path build\windows\x64\runner\Release\* -DestinationPath $zip
        Write-Host "Windows app ready: $zip (unzip and run cipher_desk.exe)" -ForegroundColor Green
    }
} finally {
    Pop-Location
    # The key passwords stay only in key.properties (ignored by git).
}

Step "Done"
Write-Host "Your files are in: $Out" -ForegroundColor Green
Write-Host "Copy the APK to your phone (Google Drive, USB cable or WhatsApp to yourself) and open it to install."
Stop-Transcript | Out-Null
Start-Process explorer.exe $Out
