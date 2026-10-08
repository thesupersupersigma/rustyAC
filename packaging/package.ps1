# SPDX-License-Identifier: MIT OR Apache-2.0

# Packs a release: rustyAC-vX.Y.Z-windows-x64.zip and its .sha256, from the release build that is
# already in target\release. Run from anywhere:
#
#   cargo build --release --workspace
#   packaging\package.ps1 -Version 0.14.0 -OutDir dist
#
# The zip holds rustyac.exe, the README, the license files and HOW_TO_RUN.txt, and nothing else:
# the list below is the whole content, and the zip is read back and compared with it. No Assetto
# Corsa file is ever packed.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$OutDir = "dist"
)

$ErrorActionPreference = "Stop"

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "the version must look like 0.14.0, not '$Version'"
}

$root = Split-Path -Parent $PSScriptRoot
$tag = "v$Version"
$name = "rustyAC-$tag-windows-x64"

# source (from the repo's root) -> name in the zip. ODE's license keeps its path, which is the
# one LICENSING.md names.
$files = [ordered]@{
    "target/release/rustyac.exe"     = "rustyac.exe"
    "README.md"                      = "README.md"
    "LICENSING.md"                   = "LICENSING.md"
    "LICENSE-GPL"                    = "LICENSE-GPL"
    "LICENSE-MIT"                    = "LICENSE-MIT"
    "LICENSE-APACHE"                 = "LICENSE-APACHE"
    "crates/rustyac-ode/LICENSE-ODE" = "crates/rustyac-ode/LICENSE-ODE"
}

if (-not [System.IO.Path]::IsPathRooted($OutDir)) {
    $OutDir = Join-Path $root $OutDir
}
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$zip = Join-Path $OutDir "$name.zip"
$sum = "$zip.sha256"
foreach ($old in @($zip, $sum)) {
    if (Test-Path $old) { Remove-Item -Force $old }
}

foreach ($source in $files.Keys) {
    if (-not (Test-Path -PathType Leaf (Join-Path $root $source))) {
        throw "$source is missing (build first: cargo build --release --workspace)"
    }
}

# the text file gets Windows line ends, whatever git checked out
$text = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot "HOW_TO_RUN.txt"))
$text = $text.Replace("@VERSION@", $Version).Replace("@TAG@", $tag)
$text = $text.Replace("`r`n", "`n").Replace("`n", "`r`n")

# written entry by entry (not with Compress-Archive, whose older versions put backslashes into
# the names of files in folders)
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$best = [System.IO.Compression.CompressionLevel]::Optimal
$archive = [System.IO.Compression.ZipFile]::Open($zip, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($source in $files.Keys) {
        $path = (Resolve-Path (Join-Path $root $source)).Path
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $path, $files[$source], $best) | Out-Null
    }
    $stream = $archive.CreateEntry("HOW_TO_RUN.txt", $best).Open()
    $writer = New-Object System.IO.StreamWriter($stream, (New-Object System.Text.UTF8Encoding($false)))
    $writer.Write($text)
    $writer.Dispose()
} finally {
    $archive.Dispose()
}

# read the zip back: exactly the expected names
$archive = [System.IO.Compression.ZipFile]::OpenRead($zip)
try {
    $packed = @($archive.Entries | ForEach-Object { $_.FullName } | Sort-Object)
} finally {
    $archive.Dispose()
}
$expected = @(@($files.Values) + "HOW_TO_RUN.txt" | Sort-Object)
if (Compare-Object $expected $packed) {
    throw "the zip does not hold what it should: expected [$($expected -join ', ')], found [$($packed -join ', ')]"
}

# the same line `sha256sum` writes, so `sha256sum -c` checks it
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
[System.IO.File]::WriteAllText($sum, "$hash  $name.zip`n", (New-Object System.Text.ASCIIEncoding))

Write-Host "packed $zip"
foreach ($entry in $packed) { Write-Host "  $entry" }
Write-Host "sha256 $hash"
