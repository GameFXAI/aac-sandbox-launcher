# Builds a launcher release: the exe, its signed update manifest and the testers' installer.
#
#   .\installer\release.ps1 -Version 0.2.1 -KeyFile <update signing key file>
#   .\installer\release.ps1 -Version 0.2.1 -KeyFile <key> -NoAuthenticode      unsigned exe and installer on purpose
#   .\installer\release.ps1 -Version 0.2.1 -TestPfx test.pfx                    signing mechanics with a test certificate
#
# Steps:
#   1. Cargo.toml must already say this version; cargo test, cargo build --release.
#   2. Authenticode (Azure Artifact Signing, installer\sign.ps1): the exe is signed before anything is made from it,
#      because the update manifest names the exe's SHA-256. When signing is not set up (docs/code-signing.md), the
#      release is made unsigned, as before, and says so.
#   3. release\aac-sandbox-launcher-<version>.exe, release\manifest-<version>.json and its ed25519 signature (.sig),
#      with -KeyFile (the update key; its path is passed on, its contents never printed).
#   4. installer\out\AAReplaySetup.exe with the same exe inside; Inno Setup signs the setup and its uninstaller through
#      sign.ps1 when signing is set up.
#   5. release\AAReplaySetup-<version>.exe (that installer) and release\setup-<version>.json (+ .sig): what the
#      bootstrapper fetches. The bootstrapper (bootstrap\, the file people download) is not rebuilt.
# Uploading stays a separate, deliberate step (README: Updating the launcher itself).
param(
    [Parameter(Mandatory)][string]$Version,
    [string]$KeyFile,
    [switch]$NoAuthenticode,
    [string]$TestPfx,
    [string]$BaseUrl = 'https://77-42-46-123.sslip.io/launcher'
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $repo
$sign = Join-Path $PSScriptRoot 'sign.ps1'

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'the version is like 0.2.1' }
$cargoVersion = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if ($cargoVersion -ne $Version) { throw "Cargo.toml says $cargoVersion, not ${Version}: bump it first" }
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'no Inno Setup 6 (winget install -e --id JRSoftware.InnoSetup)' }

# 1. build
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH" }
cargo test --release
if ($LASTEXITCODE -ne 0) { throw 'tests failed' }
cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'build failed' }
$exe = Join-Path $repo 'target\release\aac-sandbox-launcher.exe'
# cargo's exe is a hard link to its own cached copy in target\release\deps: signing it in place would sign that copy
# too, and every later build would hand the signed file back. A copy of its own first.
Copy-Item $exe "$exe.own" -Force; Remove-Item $exe -Force; Move-Item "$exe.own" $exe

# 2. Authenticode
$signArgs = @()
if ($TestPfx) { $signArgs = @('-TestPfx', (Resolve-Path $TestPfx).Path) }
$signed = $false
if ($NoAuthenticode) {
    Write-Output 'Authenticode: off (-NoAuthenticode)'
} else {
    $state = & powershell -NoProfile -ExecutionPolicy Bypass -File $sign -Status @signArgs
    Write-Output "Authenticode: $state"
    if ($state -notmatch '^signing is not set up') {
        & powershell -NoProfile -ExecutionPolicy Bypass -File $sign -Require -Files $exe @signArgs
        if ($LASTEXITCODE -ne 0) { throw 'signing the exe failed' }
        $signed = $true
    }
}

# 3. the update files
$rel = Join-Path $repo 'release'
New-Item -ItemType Directory -Force $rel | Out-Null
$name = "aac-sandbox-launcher-$Version.exe"
Copy-Item $exe (Join-Path $rel $name) -Force
$manifest = Join-Path $rel "manifest-$Version.json"
& $exe manifest make (Join-Path $rel $name) $Version "$BaseUrl/$name" $manifest | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'manifest make failed' }
if ($KeyFile) {
    & $exe manifest sign $manifest $KeyFile | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'manifest sign failed' }
    Write-Output "update files: release\$name, release\manifest-$Version.json (+ .sig)"
} else {
    Write-Output "update files: release\$name, release\manifest-$Version.json (NOT signed: no -KeyFile, launchers will refuse it)"
}

# 4. the installer
$isccArgs = @('/Q', "/DAppVersion=$Version")
if ($signed) {
    $tool = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$sign`" -Require $($signArgs -join ' ') -Files `$f"
    $isccArgs += @('/DSign', "/Saareplay=$tool")
}
& $iscc @isccArgs 'installer\aac-sandbox-launcher.iss'
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed' }
$setup = Join-Path $repo 'installer\out\AAReplaySetup.exe'
foreach ($f in $exe, $setup) {
    $s = Get-AuthenticodeSignature $f
    $who = if ($s.SignerCertificate) { $s.SignerCertificate.Subject } else { 'unsigned' }
    Write-Output ("{0}: {1} ({2})" -f (Split-Path $f -Leaf), $who, $s.Status)
}
Write-Output "installer: installer\out\AAReplaySetup.exe ($((Get-Item $setup).Length) bytes)"

# 5. what the bootstrapper (bootstrap\, the AAReplaySetup.exe people download) fetches: this release's installer
#    under its own name and the signed setup.json naming it. The bootstrapper itself is not rebuilt here: it stays
#    the same file across releases (docs/bootstrapper.md).
$inner = "AAReplaySetup-$Version.exe"
Copy-Item $setup (Join-Path $rel $inner) -Force
$hash = (Get-FileHash (Join-Path $rel $inner) -Algorithm SHA256).Hash.ToLowerInvariant()
$size = (Get-Item (Join-Path $rel $inner)).Length
$setupJson = Join-Path $rel "setup-$Version.json"
$text = "{`n `"setup`": {`n  `"version`": `"$Version`",`n  `"url`": `"$BaseUrl/setup/$inner`",`n  `"size`": $size,`n  `"sha256`": `"$hash`"`n }`n}`n"
[System.IO.File]::WriteAllText($setupJson, $text, (New-Object System.Text.UTF8Encoding $false))
if ($KeyFile) {
    & $exe manifest sign $setupJson $KeyFile | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'setup.json sign failed' }
    Write-Output "bootstrapper files: release\$inner, release\setup-$Version.json (+ .sig) -> launcher/setup/ and launcher/setup.json"
} else {
    Write-Output "bootstrapper files: release\$inner, release\setup-$Version.json (NOT signed: no -KeyFile)"
}
