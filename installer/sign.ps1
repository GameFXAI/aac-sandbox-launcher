# Authenticode signing with Azure Artifact Signing (formerly Trusted Signing), optional.
#
#   .\installer\sign.ps1 -Files a.exe,b.exe            sign when signing is set up; otherwise say so and do nothing
#   .\installer\sign.ps1 -Files a.exe -Require         fail when signing is not set up (Inno Setup's SignTool uses this)
#   .\installer\sign.ps1 -Status                       say whether signing is set up, and with what
#   .\installer\sign.ps1 -Files a.exe -TestPfx t.pfx   sign with a local test certificate instead (checks the mechanics
#                                                      only; its password in $env:AAREPLAY_TEST_PFX_PASSWORD)
#
# Set up (docs/code-signing.md): a folder outside every repository, by default
# %LOCALAPPDATA%\AAEmuSandbox-dev\signing (or $env:AAREPLAY_SIGNING_DIR), holding
#   metadata.json   {"Endpoint": "https://weu.codesigning.azure.net", "CodeSigningAccountName": "...",
#                    "CertificateProfileName": "..."}
# plus the Artifact Signing client tools (winget install -e --id Microsoft.Azure.ArtifactSigningClientTools, or the
# NuGet package Microsoft.ArtifactSigning.Client unpacked into that folder), and an Azure sign-in that holds the
# "Artifact Signing Certificate Profile Signer" role: `az login`, or AZURE_TENANT_ID / AZURE_CLIENT_ID /
# AZURE_CLIENT_SECRET of an app registration. Nothing secret is in metadata.json.
param([string[]]$Files = @(), [switch]$Require, [switch]$Status, [string]$TestPfx)
$ErrorActionPreference = 'Stop'

$dir = if ($env:AAREPLAY_SIGNING_DIR) { $env:AAREPLAY_SIGNING_DIR } else { Join-Path $env:LOCALAPPDATA 'AAEmuSandbox-dev\signing' }
$metadata = Join-Path $dir 'metadata.json'

function Find-SignTool {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $found = Get-ChildItem $kits -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^10\.0\.(\d+)\.' -and [int]$Matches[1] -ge 22621 } |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $found) { $found = (Get-Command signtool.exe -ErrorAction SilentlyContinue).Source }
    return $found
}

function Find-Dlib {
    if ($env:AAREPLAY_SIGNING_DLIB -and (Test-Path $env:AAREPLAY_SIGNING_DLIB)) { return $env:AAREPLAY_SIGNING_DLIB }
    $roots = @($dir, (Join-Path $env:LOCALAPPDATA 'Microsoft'), (Join-Path $env:ProgramFiles 'Microsoft'),
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft')) | Where-Object { $_ -and (Test-Path $_) }
    foreach ($r in $roots) {
        $hit = Get-ChildItem $r -Recurse -Filter 'Azure.CodeSigning.Dlib.dll' -ErrorAction SilentlyContinue |
            Where-Object { $_.DirectoryName -match '\\x64$' } | Select-Object -First 1
        if ($hit) { return $hit.FullName }
    }
    return $null
}

$signtool = Find-SignTool
$why = $null
if (-not $signtool) { $why = 'no signtool.exe (Windows SDK 10.0.22621 or newer)' }
elseif ($TestPfx) { if (-not (Test-Path $TestPfx)) { $why = "no test certificate $TestPfx" } }
elseif (-not (Test-Path $metadata)) { $why = "no $metadata" }
else {
    $dlib = Find-Dlib
    if (-not $dlib) { $why = 'no Azure.CodeSigning.Dlib.dll (Artifact Signing client tools)' }
    $m = Get-Content $metadata -Raw | ConvertFrom-Json
    foreach ($k in 'Endpoint', 'CodeSigningAccountName', 'CertificateProfileName') {
        if (-not $m.$k) { $why = "metadata.json has no $k" }
    }
    if ($m.Endpoint -and $m.Endpoint -notmatch '^https://[a-z0-9]+\.codesigning\.azure\.net/?$') { $why = "metadata.json's Endpoint is not an Artifact Signing endpoint" }
}

if ($Status) {
    if ($why) { "signing is not set up: $why. Releases ship unsigned." }
    elseif ($TestPfx) { "signing with the test certificate $TestPfx ($signtool)" }
    else { "signing is set up: account $($m.CodeSigningAccountName), profile $($m.CertificateProfileName), $($m.Endpoint); $signtool; $dlib" }
    exit 0
}
if ($why) {
    if ($Require) { Write-Error "cannot sign: $why" }
    "not signed: $why"
    exit 0
}

foreach ($f in $Files) {
    if (-not (Test-Path $f)) { Write-Error "no file $f" }
    $common = @('sign', '/fd', 'SHA256', '/td', 'SHA256')
    if ($TestPfx) {
        # a local self-signed certificate has no timestamp server: only the mechanics are checked
        $pw = if ($env:AAREPLAY_TEST_PFX_PASSWORD) { @('/p', $env:AAREPLAY_TEST_PFX_PASSWORD) } else { @() }
        $out = & $signtool @common /f $TestPfx @pw $f 2>&1
    } else {
        $out = & $signtool @common /tr 'http://timestamp.acs.microsoft.com' /dlib $dlib /dmdf $metadata $f 2>&1
    }
    if ($LASTEXITCODE -ne 0) { $out | Out-String | Write-Host; Write-Error "signtool failed on $f ($LASTEXITCODE)" }
    $sig = Get-AuthenticodeSignature $f
    if (-not $sig.SignerCertificate) { Write-Error "$f has no signature after signing" }
    "signed $f ($($sig.SignerCertificate.Subject); $($sig.Status))"
}
