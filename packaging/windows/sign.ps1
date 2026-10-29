# Signs Windows files with Authenticode when a code-signing certificate is
# provided through GitHub Actions secrets (S8, S12):
#   WINDOWS_CERT_PFX_BASE64  base64 of the .pfx    WINDOWS_CERT_PASSWORD  its password
# Without them the files stay unsigned and a warning is printed (the release
# notes must then say "unsigned").
#   pwsh packaging/windows/sign.ps1 <file> [<file> ...]
$ErrorActionPreference = 'Stop'
if (-not $env:WINDOWS_CERT_PFX_BASE64) {
  Write-Output "::warning::No code-signing certificate configured; Windows files are UNSIGNED."
  exit 0
}
$pfx = Join-Path ([IO.Path]::GetTempPath()) "rdpv-sign.pfx"
try {
  [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERT_PFX_BASE64))
  $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
  if (-not $signtool) { throw "signtool.exe not found" }
  foreach ($file in $args) {
    & $signtool.FullName sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 `
      /f $pfx /p $env:WINDOWS_CERT_PASSWORD $file
    if ($LASTEXITCODE -ne 0) { throw "signing failed: $file" }
  }
} finally {
  if (Test-Path $pfx) { Remove-Item -Force $pfx }
}
