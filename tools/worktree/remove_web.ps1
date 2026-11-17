# SPDX-License-Identifier: GPL-3.0-or-later
#
# Takes away the worktree make_web.ps1 made: first every junction in it, as a link (what it
# points at is never entered and never deleted), then a check that none is left, then
# "git worktree remove" without --force. Refuses while the worktree has changes that are not
# committed. The branch stays.
#
#   powershell -ExecutionPolicy Bypass -File tools\worktree\remove_web.ps1
param([string]$Path)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $Path) { $Path = Join-Path (Split-Path $repo -Parent) 'rustyAC-web' }
if (-not (Test-Path -LiteralPath (Join-Path $Path '.git') -PathType Leaf)) { Write-Error "$Path is not a worktree (no .git file in it): nothing done" }
$Path = (Resolve-Path -LiteralPath $Path).Path

$changes = git -C $Path status --porcelain
if ($LASTEXITCODE -ne 0) { Write-Error "git status failed in ${Path}: nothing done" }
if ($changes) { Write-Error "$Path has changes that are not committed: nothing done`n$($changes -join "`n")" }

# every folder that is a link, found without ever going into one
function Get-Links([string]$root) {
    $todo = New-Object System.Collections.Stack
    $todo.Push((New-Object IO.DirectoryInfo $root))
    while ($todo.Count) {
        foreach ($d in $todo.Pop().GetDirectories()) {
            if ($d.Attributes -band [IO.FileAttributes]::ReparsePoint) { $d } else { $todo.Push($d) }
        }
    }
}
foreach ($link in @(Get-Links $Path)) {
    $link.Delete()   # not recursive: the link goes, its target stays
    Write-Output "unlinked $($link.FullName)"
}
$left = @(Get-Links $Path)
if ($left.Count) { Write-Error "links are left, the worktree stays:`n$($left.FullName -join "`n")" }

git -C $repo worktree remove $Path
if ($LASTEXITCODE -ne 0) { Write-Error 'git worktree remove refused: the worktree stays (its links are gone; make them again by hand or commit and run this again)' }
Write-Output "removed $Path"
Write-Output 'its branch is still there: delete it only after it is merged (git branch -d <branch>; -d refuses an unmerged one)'
