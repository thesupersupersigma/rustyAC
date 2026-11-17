# SPDX-License-Identifier: GPL-3.0-or-later
#
# Makes a second checkout for the browser work: the git worktree ..\rustyAC-web on the new
# branch "web" (from master), and in it junctions to this folder's git-ignored things that
# builds and proofs read, so nothing big is copied. Not linked: target\ (each checkout builds
# its own), brainstorm.md, the tools of the reverse engineering. Take the worktree away only
# with remove_web.ps1: a plain recursive delete would follow the junctions into this folder.
#
#   powershell -ExecutionPolicy Bypass -File tools\worktree\make_web.ps1
#
# -Path, -Branch and -Links are for trying the scripts on a throwaway worktree.
param(
    [string]$Path,
    [string]$Branch = 'web',
    [string[]]$Links = @('re', 'ignored', 'prompts', 'IDA', 'oracle', 'cardata')
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $Path) { $Path = Join-Path (Split-Path $repo -Parent) 'rustyAC-web' }
if (Test-Path -LiteralPath $Path) { Write-Error "$Path is there already: nothing done (remove_web.ps1 takes a worktree away)" }

git -C $repo worktree add -b $Branch $Path master
if ($LASTEXITCODE -ne 0) { Write-Error "git worktree add failed: nothing linked" }
$Path = (Resolve-Path -LiteralPath $Path).Path

# -File hands a,b,c over as one string
foreach ($name in ($Links -split ',')) {
    $from = Join-Path $repo $name
    $to = Join-Path $Path $name
    if (-not (Test-Path -LiteralPath $from -PathType Container)) { Write-Output "not linked, not in this folder: $name"; continue }
    if (Test-Path -LiteralPath $to) { Write-Output "not linked, the checkout has it: $name"; continue }
    New-Item -ItemType Directory -Force -Path (Split-Path $to -Parent) | Out-Null
    New-Item -ItemType Junction -Path $to -Target $from | Out-Null
    Write-Output "linked $name -> $from"
}
# a file cannot be a junction: the pack list web_pack takes by default is copied
$pack = Join-Path $repo 'web\my_pack.toml'
if (Test-Path -LiteralPath $pack) { Copy-Item -LiteralPath $pack -Destination (Join-Path $Path 'web\my_pack.toml'); Write-Output 'copied web\my_pack.toml' }

Write-Output "the worktree is $Path on branch $Branch"
Write-Output 'while it exists: at most 4 build jobs in each folder (CARGO_BUILD_JOBS=4); proof runs take turns by themselves'
