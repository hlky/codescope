param(
    [string]$CodexHome = "$env:USERPROFILE\.codex",
    [string]$BinaryDir = "$env:USERPROFILE\.codex\bin"
)

$ErrorActionPreference = "Stop"

cargo build --release

New-Item -ItemType Directory -Force $BinaryDir | Out-Null
Copy-Item -Force target\release\codescope.exe (Join-Path $BinaryDir "codescope.exe")

$skillDir = Join-Path $CodexHome "skills\extract-function"
New-Item -ItemType Directory -Force $skillDir | Out-Null
Copy-Item -Force skill\SKILL.md (Join-Path $skillDir "SKILL.md")

Write-Host "Installed codescope.exe to $BinaryDir"
Write-Host "Installed extract-function skill to $skillDir"
Write-Host "Ensure $BinaryDir is on PATH before using the skill."
