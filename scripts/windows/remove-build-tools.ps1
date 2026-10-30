<#
Cipher Desk: remove the build tools that build-on-this-pc.ps1 installed.

  powershell -ExecutionPolicy Bypass -File .\remove-build-tools.ps1

Keeps: the "Cipher Desk build" folder on your Desktop (APK and Windows app),
your key files, and the .android folder in your user folder.
Add -KeepGit to keep Git installed.
#>
param([switch]$KeepGit)

$ErrorActionPreference = "Continue"
$ProgressPreference = "SilentlyContinue"

function Step($text) { Write-Host ""; Write-Host "==== $text ====" -ForegroundColor Cyan }
function Remove-Tool($id) {
    Write-Host "Removing $id (Windows may ask for permission)..."
    winget.exe uninstall --id $id -e --silent --accept-source-agreements
    if ($LASTEXITCODE -ne 0) { Write-Host "  (not installed, or already removed)" -ForegroundColor DarkGray }
}
function Remove-Folder($path) {
    if (Test-Path $path) {
        Write-Host "Deleting $path ..."
        Remove-Item -Recurse -Force $path -ErrorAction SilentlyContinue
        if (Test-Path $path) { Write-Host "  Some files are in use. Restart the PC and run this again." -ForegroundColor Yellow }
    }
}

Write-Host "This removes the Cipher Desk build tools (about 20 GB)." -ForegroundColor Yellow
Write-Host "Your Desktop 'Cipher Desk build' folder and your key files are NOT touched."
$answer = Read-Host "Type YES to continue"
if ($answer -ne "YES") { Write-Host "Nothing was changed."; exit 0 }

Step "1/3 Uninstalling tools"
$rustup = "$HOME\.cargo\bin\rustup.exe"
if (Test-Path $rustup) {
    Write-Host "Removing Rust..."
    & $rustup self uninstall -y
}
Remove-Tool "Rustlang.Rustup"
Remove-Tool "Microsoft.OpenJDK.17"
Remove-Tool "Microsoft.VisualStudio.2022.BuildTools"
if (-not $KeepGit) { Remove-Tool "Git.Git" }

Step "2/3 Deleting downloaded files"
Remove-Folder "C:\cd"
Remove-Folder "$HOME\.gradle"
Remove-Folder "$env:LOCALAPPDATA\Pub"
Remove-Folder "$HOME\.cargo"
Remove-Folder "$HOME\.rustup"

Step "3/3 Developer Mode"
Write-Host "Turn 'Developer Mode' OFF in the Settings window that opens (unless you need it)."
Start-Process "ms-settings:developers"

Step "Done"
Write-Host "Kept: the 'Cipher Desk build' folder on your Desktop and your key files." -ForegroundColor Green
