<#
.SYNOPSIS
  Creates the planned-feature issues from docs/issues/*.md in the GitHub repository.

.DESCRIPTION
  Each docs/issues/NN-*.md file is one issue: the first line ("# Title") is the title, the rest
  is the body. Issues whose title already exists in the repository (open or closed) are skipped,
  so the script can be run again safely. Needs the GitHub CLI (gh) signed in.

.EXAMPLE
  scripts\create-issues.ps1 -DryRun
  scripts\create-issues.ps1
#>
param(
    [string]$Repo = "MiroM73/RusNet",
    [string]$Label = "enhancement",
    [switch]$DryRun
)
$ErrorActionPreference = "Stop"

$dir = Join-Path $PSScriptRoot "..\docs\issues"
$existing = @(gh issue list --repo $Repo --state all --limit 1000 --json title |
    ConvertFrom-Json | ForEach-Object { $_.title })
$utf8 = New-Object System.Text.UTF8Encoding($false)

Get-ChildItem $dir -Filter "[0-9][0-9]-*.md" | Sort-Object Name | ForEach-Object {
    $lines = Get-Content $_.FullName -Encoding UTF8
    $title = $lines[0] -replace '^#\s*', ''
    if ($existing -contains $title) {
        Write-Host "skip (exists): $title"
        return
    }
    $body = ($lines | Select-Object -Skip 2) -join "`n"
    if ($DryRun) {
        Write-Host "would create: $title"
        return
    }
    $tmp = [System.IO.Path]::GetTempFileName()
    try {
        [System.IO.File]::WriteAllText($tmp, $body, $utf8)
        gh issue create --repo $Repo --title $title --body-file $tmp --label $Label
    }
    finally {
        Remove-Item $tmp -ErrorAction SilentlyContinue
    }
}
