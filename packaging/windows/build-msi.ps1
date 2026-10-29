# Builds the Windows installer (MSI) from a Flutter Windows release bundle.
#   pwsh packaging/windows/build-msi.ps1 -BundleDir <dir> -Version 1.2.3 -OutDir <dir>
# Needs WiX Toolset v5:  dotnet tool install --global wix --version 5.0.2
#                        wix extension add -g WixToolset.UI.wixext/5.0.2
param(
  [Parameter(Mandatory)][string]$BundleDir,
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must look like 1.2.3, got '$Version'" }
$repo = (Resolve-Path "$PSScriptRoot\..\..").Path
$bundle = (Resolve-Path $BundleDir).Path
if (-not (Test-Path "$bundle\secure_rdp_viewer.exe")) { throw "secure_rdp_viewer.exe not found in $bundle" }
if (Test-Path "$bundle\portable.txt") { throw "The installer must not contain portable.txt" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$out = (Resolve-Path $OutDir).Path

# The licence page shows the project's LICENSE file (converted to RTF).
$text = Get-Content "$repo\LICENSE" -Raw
$escaped = $text.Replace('\', '\\').Replace('{', '\{').Replace('}', '\}') -replace "`r?`n", "\par`r`n"
$rtf = "{\rtf1\ansi\deff0{\fonttbl{\f0 Consolas;}}\f0\fs16 $escaped}"
$license = Join-Path $out 'License.rtf'
Set-Content -Path $license -Value $rtf -Encoding ascii

$msi = Join-Path $out "SecureRdpViewer-$Version-x64.msi"
wix build "$PSScriptRoot\SecureRdpViewer.wxs" -arch x64 -ext WixToolset.UI.wixext `
  -d "Version=$Version" -d "BundleDir=$bundle" -d "RepoDir=$repo" -d "LicenseRtf=$license" `
  -o $msi
if ($LASTEXITCODE -ne 0) { throw "wix build failed" }
Remove-Item $license
Write-Output $msi
