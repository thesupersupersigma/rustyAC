# SPDX-License-Identifier: GPL-3.0-or-later
#
# Removes re/csp/fakeac/ (made by fakeac_csp.ps1): first the junctions themselves, never what
# they point at, then the copies.
$f = Join-Path $PSScriptRoot '..\..\re\csp\fakeac'
foreach ($d in $f, "$f\system") {
    if (Test-Path $d) { Get-ChildItem $d -Force | Where-Object { $_.LinkType -eq 'Junction' } | ForEach-Object { $_.Delete() } }
}
if (Test-Path $f) { Remove-Item $f -Recurse -Force }
