param(
    [string]$CodexHome = "$env:USERPROFILE\.codex",
    [string]$BinaryDir = "$env:USERPROFILE\.codex\bin",
    [string]$Repo = "hlky/codescope",
    [string]$Version = "latest",
    [switch]$FromSource
)

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force $BinaryDir | Out-Null

$installedFromRelease = $false
if (-not $FromSource) {
    try {
        $releaseUri = if ($Version -eq "latest") {
            "https://api.github.com/repos/$Repo/releases/latest"
        } else {
            "https://api.github.com/repos/$Repo/releases/tags/$Version"
        }
        $release = Invoke-RestMethod -Uri $releaseUri
        $asset = $release.assets | Where-Object { $_.name -eq "codescope-x86_64-pc-windows-msvc.zip" } | Select-Object -First 1
        if ($null -eq $asset) {
            throw "release asset codescope-x86_64-pc-windows-msvc.zip not found"
        }
        $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("codescope-" + [System.Guid]::NewGuid())
        New-Item -ItemType Directory -Force $tmp | Out-Null
        $archive = Join-Path $tmp $asset.name
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archive
        Expand-Archive -Path $archive -DestinationPath $tmp -Force
        Copy-Item -Force (Join-Path $tmp "codescope.exe") (Join-Path $BinaryDir "codescope.exe")
        Remove-Item -Recurse -Force $tmp
        $installedFromRelease = $true
    } catch {
        Write-Warning "Release install failed: $_"
        Write-Warning "Falling back to local cargo build. Pass -FromSource to skip release lookup."
    }
}

if (-not $installedFromRelease) {
    cargo build --release
    Copy-Item -Force target\release\codescope.exe (Join-Path $BinaryDir "codescope.exe")
}

$skillDir = Join-Path $CodexHome "skills\extract-function"
New-Item -ItemType Directory -Force $skillDir | Out-Null
Copy-Item -Force skill\SKILL.md (Join-Path $skillDir "SKILL.md")

Write-Host "Installed codescope.exe to $BinaryDir"
Write-Host "Installed extract-function skill to $skillDir"
Write-Host "Ensure $BinaryDir is on PATH before using the skill."
