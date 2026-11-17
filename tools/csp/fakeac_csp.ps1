# SPDX-License-Identifier: GPL-3.0-or-later
#
# Task C0: an AC root for CSP oracle runs where CSP is the frozen build (docs/csp/version.md),
# whatever Content Manager does to the real install. Like re/scratch/task46c/fakeac.ps1: junctions
# to the AC folder's directories and system/cfg copied. Unlike it: extension/ copied from the
# frozen folder, cache/ a new empty folder, dwrite.dll from the frozen folder, acs.exe + acs.pdb
# copied (CSP resolves acs.exe's functions by name through the PDB next to the exe).
# Makes re/csp/fakeac/. Remove only with fakeac_csp_rm.ps1 (a recursive delete follows junctions).
$ErrorActionPreference = 'Stop'
$ac = 'C:\Program Files (x86)\Steam\steamapps\common\assettocorsa'
$csp = Join-Path $PSScriptRoot '..\..\ignored\CSP\lights-patch-v0.3.2-preview2'
$f = Join-Path $PSScriptRoot '..\..\re\csp\fakeac'
if (Test-Path $f) { Write-Error "$f exists: remove it with fakeac_csp_rm.ps1 first" }
if (-not (Test-Path "$csp\dwrite.dll")) { Write-Error "frozen CSP not found in $csp" }
function J($path, $target) { New-Item -ItemType Junction -Path $path -Target $target | Out-Null }
New-Item -ItemType Directory "$f\system", "$f\cache" | Out-Null
Get-ChildItem $ac -Directory | Where-Object { $_.Name -notin 'system', 'extension', 'cache' } | ForEach-Object { J "$f\$($_.Name)" $_.FullName }
Get-ChildItem "$ac\system" -Directory | Where-Object { $_.Name -ne 'cfg' } | ForEach-Object { J "$f\system\$($_.Name)" $_.FullName }
Copy-Item "$ac\system\cfg" "$f\system\cfg" -Recurse
Copy-Item "$csp\extension" "$f\extension" -Recurse
Copy-Item "$csp\dwrite.dll", "$ac\acs.exe", "$ac\acs.pdb" $f
