# Builds the Windows portable ZIP (SA-10) from a Flutter Windows release bundle.
#   pwsh packaging/windows/build-portable.ps1 -BundleDir <dir> -Version 1.2.3 -OutDir <dir>
# portable.txt next to the .exe makes the app keep connections, settings and
# trusted certificates in a "data" folder beside it. Passwords are never written
# there; if saved, they go to the vault of the computer being used.
param(
  [Parameter(Mandatory)][string]$BundleDir,
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
$bundle = (Resolve-Path $BundleDir).Path
if (-not (Test-Path "$bundle\secure_rdp_viewer.exe")) { throw "secure_rdp_viewer.exe not found in $bundle" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$stage = Join-Path ([IO.Path]::GetTempPath()) "SecureRDPViewer-$Version-portable"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
Copy-Item -Recurse $bundle $stage
if (Test-Path "$stage\data\connections.json") { throw "The bundle must not contain user data" }

@"
This file makes Secure RDP Viewer run as a PORTABLE copy.
Connections, settings and trusted certificates are kept in the "data" folder
next to this file. Passwords are never stored on the drive.
Delete this file to use the normal folders of the signed-in Windows user instead.
"@ | Set-Content -Path "$stage\portable.txt" -Encoding utf8

@"
Secure RDP Viewer $Version - portable copy for Windows (64-bit)

Run secure_rdp_viewer.exe. No installation or administrator rights are needed.

- Your connections and settings are saved in the "data" folder next to the app.
  Keep the drive safe: anyone with it can see your computer names and usernames
  (never your passwords).
- "Save password" is off by default. If you turn it on, the password is stored in
  the Windows Credential Manager of the computer you are using, not on the drive.
  Use Settings > Remove all saved passwords before leaving a computer you don't own.
- Read docs/USER_GUIDE.md on the project page for more.
"@ | Set-Content -Path "$stage\README-portable.txt" -Encoding utf8

$zip = Join-Path (Resolve-Path $OutDir).Path "SecureRdpViewer-$Version-windows-x64-portable.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $stage -DestinationPath $zip
Remove-Item -Recurse -Force $stage
Write-Output $zip
