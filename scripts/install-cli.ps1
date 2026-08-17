# Install the Token Guard CLI from GitHub Releases on Windows.
# Usage: irm https://raw.githubusercontent.com/QQSHI13/tokenguard/main/scripts/install-cli.ps1 | iex
#        iex "& { $(irm https://raw.githubusercontent.com/QQSHI13/tokenguard/main/scripts/install-cli.ps1) } -Beta"
#        iex "& { $(irm https://raw.githubusercontent.com/QQSHI13/tokenguard/main/scripts/install-cli.ps1) } -Version v0.2.0-beta.3"

param(
    [string]$Version = "",
    [string]$Dest = "$env:LOCALAPPDATA\Programs\tokenguard",
    [switch]$Beta
)

$Repo = "QQSHI13/tokenguard"
$ErrorActionPreference = "Stop"

# Detect architecture.
$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "x86_64" }
    "ARM64" { "aarch64" }
    default {
        Write-Error "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE"
    }
}

$asset = "tokenguard-windows-${arch}.exe"
$binary = "tokenguard.exe"

if (-not $Version) {
    if ($Beta) {
        $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases"
        $prerelease = $releases | Where-Object { $_.tag_name -like '*-*' } | Select-Object -First 1
        $Version = $prerelease.tag_name
        if (-not $Version) {
            Write-Error "Could not determine latest pre-release"
        }
    } else {
        $latest = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $Version = $latest.tag_name
        if (-not $Version) {
            Write-Error "Could not determine latest stable release"
        }
    }
}

$url = "https://github.com/$Repo/releases/download/$Version/$asset"

Write-Host "Installing Token Guard CLI $Version for windows/$arch..."
Write-Host "  $url"

New-Item -ItemType Directory -Force -Path $Dest | Out-Null
$tmp = Join-Path $env:TEMP $binary

try {
    Invoke-RestMethod -Uri $url -OutFile $tmp
} catch {
    Write-Error "Download failed. The asset may not exist for this version/architecture."
}

if ((Get-Item $tmp).Length -eq 0) {
    Write-Error "Downloaded file is empty."
}

Move-Item -Path $tmp -Destination (Join-Path $Dest $binary) -Force

# Add to user PATH if missing.
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$Dest*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$Dest", "User")
    Write-Host "Added $Dest to your user PATH. Restart your terminal to use tokenguard."
}

Write-Host "Installed to $Dest\$binary"
