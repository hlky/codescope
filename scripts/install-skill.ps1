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
    $tmp = $null
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
        $checksums = $release.assets | Where-Object { $_.name -eq "SHA256SUMS" } | Select-Object -First 1
        if ($null -ne $checksums) {
            $checksumFile = Join-Path $tmp "SHA256SUMS"
            Invoke-WebRequest -Uri $checksums.browser_download_url -OutFile $checksumFile
            $expected = Get-Content $checksumFile |
                Where-Object { $_ -match "\s$([regex]::Escape($asset.name))$" } |
                ForEach-Object { ($_ -split "\s+")[0].ToLowerInvariant() } |
                Select-Object -First 1
            if (-not $expected) {
                throw "checksum for $($asset.name) not found in SHA256SUMS"
            }
            $actual = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
            if ($actual -ne $expected) {
                throw "checksum mismatch for $($asset.name)"
            }
        } else {
            Write-Warning "Release does not include SHA256SUMS; skipping checksum verification."
        }
        Expand-Archive -Path $archive -DestinationPath $tmp -Force
        Copy-Item -Force (Join-Path $tmp "codescope.exe") (Join-Path $BinaryDir "codescope.exe")
        $installedFromRelease = $true
    } catch {
        Write-Warning "Release install failed: $_"
        Write-Warning "Falling back to local cargo build. Pass -FromSource to skip release lookup."
    } finally {
        if ($null -ne $tmp -and (Test-Path $tmp)) {
            Remove-Item -Recurse -Force $tmp
        }
    }
}

if (-not $installedFromRelease) {
    cargo build --release
    Copy-Item -Force target\release\codescope.exe (Join-Path $BinaryDir "codescope.exe")
}

$skillDir = Join-Path $CodexHome "skills\codescope"
New-Item -ItemType Directory -Force $skillDir | Out-Null
Copy-Item -Force skill\SKILL.md (Join-Path $skillDir "SKILL.md")
$legacySkillDir = Join-Path $CodexHome "skills\extract-function"
if (Test-Path $legacySkillDir) {
    Remove-Item -Recurse -Force $legacySkillDir
}

Write-Host "Installed codescope.exe to $BinaryDir"
Write-Host "Installed codescope skill to $skillDir"
Write-Host "Ensure $BinaryDir is on PATH before using the skill."
