<#
.SYNOPSIS
Sets up everything needed to build and run bdo-discord-rpc on Windows.

.DESCRIPTION
Installs what is missing and skips what is already there, so it is safe to run
again: rustup with clippy and rustfmt, the MSVC build tools, the Npcap SDK with
LIBPCAP_LIBDIR, and optionally Npcap itself, which only capturing needs.
Open a new terminal afterwards so the changed environment is picked up.

.EXAMPLE
./scripts/setup.ps1
./scripts/setup.ps1 -SkipNpcap
#>
param(
    [switch]$SkipNpcap
)

$ErrorActionPreference = "Stop"

# Keep in step with .github/workflows/build.yml.
$SdkUrl = "https://npcap.com/dist/npcap-sdk-1.16.zip"
$SdkHash = "F0A8BE7778EE3AE1B99BBBECB27A3FF0F6C111A4093F1C78C5C5A099607184DB"
$LibpcapVer = "1.10.6"

function Update-Path {
    $env:Path = @(
        [Environment]::GetEnvironmentVariable("Path", "Machine"),
        [Environment]::GetEnvironmentVariable("Path", "User")
    ) -join ";"
}

function Install-Winget($id, [string[]]$extra) {
    winget install --id $id --exact --source winget --accept-package-agreements --accept-source-agreements @extra
    if ($LASTEXITCODE -ne 0) { throw "winget could not install $id" }
    Update-Path
}

function Set-UserVariable($name, $value) {
    [Environment]::SetEnvironmentVariable($name, $value, "User")
    Set-Item "env:$name" $value
    "  $name = $value"
}

"Rust"
if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Install-Winget Rustlang.Rustup
}
# Install only, never update a toolchain that is already there.
if (-not (rustup toolchain list | Select-String '^stable')) {
    rustup toolchain install stable --profile minimal
}
rustup component add clippy rustfmt --toolchain stable
if ($LASTEXITCODE -ne 0) { throw "rustup could not set up the stable toolchain" }

"MSVC Build Tools"
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$msvc = if (Test-Path $vswhere) {
    & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
if ($msvc) {
    "  Found at $msvc"
} else {
    Install-Winget Microsoft.VisualStudio.BuildTools @(
        "--override", "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
    )
}

"Npcap SDK"
if ($env:LIBPCAP_LIBDIR -and (Test-Path (Join-Path $env:LIBPCAP_LIBDIR "wpcap.lib"))) {
    "  Found at $env:LIBPCAP_LIBDIR"
} else {
    $sdk = Join-Path $env:LOCALAPPDATA "npcap-sdk"
    $zip = Join-Path $env:TEMP "npcap-sdk.zip"
    Invoke-WebRequest $SdkUrl -OutFile $zip
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
    if ($hash -ne $SdkHash) { throw "Npcap SDK hash mismatch: $hash" }
    Expand-Archive $zip $sdk -Force
    Remove-Item $zip
    Set-UserVariable LIBPCAP_LIBDIR (Join-Path $sdk "Lib\x64")
}

"Npcap"
$npcap = Join-Path $env:SystemRoot "System32\Npcap\wpcap.dll"
if (Test-Path $npcap) {
    "  Installed"
} elseif ($SkipNpcap) {
    "  Skipped, the app runs without it and reads the game's files instead"
} else {
    $page = Invoke-WebRequest https://npcap.com/ -UseBasicParsing
    $href = $page.Links.href | Where-Object { $_ -match '^dist/npcap-[\d.]+\.exe$' } | Select-Object -First 1
    if (-not $href) { throw "No Npcap installer link on npcap.com, install it from https://npcap.com/#download" }
    $installer = Join-Path $env:TEMP (Split-Path $href -Leaf)
    Invoke-WebRequest "https://npcap.com/$href" -OutFile $installer
    $signature = Get-AuthenticodeSignature $installer
    if ($signature.Status -ne "Valid" -or $signature.SignerCertificate.Subject -notmatch "Nmap Software") {
        throw "The Npcap installer is not signed by Nmap Software: $($signature.Status) $($signature.SignerCertificate.Subject)"
    }
    "  Running $(Split-Path $href -Leaf), follow the installer"
    Start-Process $installer -Verb RunAs -Wait
    Remove-Item $installer
}

# pcap reads the version from wpcap.dll at build time, and needs it given without Npcap.
if (-not (Test-Path $npcap) -and -not $env:LIBPCAP_VER) {
    Set-UserVariable LIBPCAP_VER $LibpcapVer
}

"Done. Open a new terminal, then run: cargo build"
