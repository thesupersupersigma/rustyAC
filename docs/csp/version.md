# The CSP build rustyAC targets

**Custom Shaders Patch 0.3.2-preview2, build 4305**, frozen on 2026-11-17 (Task C0).

rustyAC's CSP work targets this one build. Upgrading is a deliberate step taken between CSP
milestones (new frozen copy, new hashes here, addresses re-matched), never a side effect of
Content Manager updating the player's install.

The atlas in this folder ([`README.md`](README.md), [`plan.md`](plan.md) and the area files) was
made from 0.3.0-preview649 (build 4184). Its addresses are build 4184's until re-matched.

## The frozen copy

The zip as downloaded and its unpacked `dwrite.dll` + `extension/` are kept outside the repo
(CSP's files are never committed). Hashes are SHA-256.

| File | Size (bytes) | SHA-256 |
|---|---:|---|
| `lights-patch-v0.3.2-preview2.zip` | 147,562,081 | `1a9884c0c2a72e545a4c85c022b9e826ac0ca18d8f19bf5aa79874a8bbfd0af4` |
| `dwrite.dll` | 143,148,080 | `2eb09e6faaadd87bc82e1af4756a25c4e8ce45fbffc848e71bccd7c12546614f` |
| `extension/` (754 files, 71,103,399 bytes) | | `4392224ce2c2b6032df79d9a512d9e56db256c439449dd63f48702b6d5afb197` |

The `extension/` hash is the SHA-256 of a list with one line per file, `relative path,size,sha256`
(paths starting with `extension/`, forward slashes, sorted byte-wise, `LC_ALL=C sort`).

What identifies the build:

- `dwrite.dll` version resource: FileVersion `0.3.2.2`, ProductVersion `0.3.2-preview2`,
  ProductName `AC Custom Shaders Patch`, OriginalFilename `accRenderingAdv.dll`, CompanyName
  `AC Modders Team`. PE time stamp 2026-10-29 22:00 UTC.
- `extension/config/data_manifest.ini`, `[VERSION]`: `SHADERS_PATCH=0.3.2-preview2`,
  `SHADERS_PATCH_BUILD=4305`.

CSP finds `acs.exe`'s functions by name through `acs.pdb` next to the exe. The `acs.exe` 1.16.4
it attaches to is the one Vanilla was ported from:

| File | Size (bytes) | SHA-256 |
|---|---:|---|
| `acs.exe` | 22,890,776 | `0df569c840f8303f7018f7891085e3a4c22cf93fb19327c6a0b85325cea23fd1` |
| `acs.pdb` | 69,218,304 | `80f5afbba6a83d8bf2e51d26a6f69379f13870808296042fdcc9960dac6a51e4` |

## The owner's install (checked 2026-11-17)

- CSP is installed in the AC folder: `dwrite.dll` next to `acs.exe` has the frozen hash, and its
  `extension/config/data_manifest.ini` says 0.3.2-preview2, build 4305. **It matches.**
- `extension/config/general.ini` has `[BASIC] ENABLED=1`. The user override
  `Documents\Assetto Corsa\cfg\extension\general.ini` only sets `[CONTROL] RELEASE_DPAD=1`, so CSP
  is on.

The install can change whenever Content Manager updates CSP, so CSP oracle runs use their own AC
root with the frozen build: `tools/csp/fakeac_csp.ps1` makes `re/csp/fakeac/` (remove it only with
`tools/csp/fakeac_csp_rm.ps1`).

## The local disassembly database (built 2026-11-17)

Made from a copy of the frozen `dwrite.dll` in batch mode, kept local and
never committed: 560 s wall time (527 s auto-analysis), 477,029,429 bytes,
76,361 functions, 50,067 strings. The export directory names the module
`accRenderingAdv.release.dll`, as in build 4184. The start-up log string
` functions were found successfully` (`attach.md` 1.x) is at 0x182379e10,
used once, by the function at 0x18157a7c0 (build 4184: 0x14fa3a0; image
base 0x180000000).

## Resume here

Task C0 (setup), Task C1 (the in-process spike) and Task C2 (a car with CSP live) done.
[`inprocess.md`](inprocess.md) Part 3 has the current state and its own "Resume here". The rows
C3 and C4 need in the area files read `0x<4184> → 0x<4305>` (inprocess.md 3.7); other addresses
are still build 4184's.
