// SPDX-License-Identifier: MIT OR Apache-2.0
// Windows version info (Properties -> Details) for one of the release's programs, shared by the
// build.rs of rustyac-game and rustyac-launcher through include!. It writes the VERSIONINFO
// resource as a .res file, which the MSVC linker takes as an input: no rc.exe, no extra crate.

fn version_res(bin: &str, file: &str, description: &str) {
    println!("cargo:rerun-if-changed=../../packaging/version_res.rs");
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    if var("CARGO_CFG_TARGET_OS") != "windows" || var("CARGO_CFG_TARGET_ENV") != "msvc" {
        return;
    }
    let version = var("CARGO_PKG_VERSION");
    let n: Vec<u16> = version.split('.').map(|p| p.parse().expect("version must be X.Y.Z")).collect();
    assert_eq!(n.len(), 3, "version must be X.Y.Z, not {version}");
    let ms = (n[0] as u32) << 16 | n[1] as u32;
    let ls = (n[2] as u32) << 16;
    // VS_FIXEDFILEINFO: signature, struct version, file and product version, flags mask, flags,
    // VOS_NT_WINDOWS32, VFT_APP, subtype, date
    let fixed: Vec<u8> = [0xfeef04bd, 0x10000, ms, ls, ms, ls, 0x3f, 0, 0x40004, 1, 0, 0, 0]
        .iter()
        .flat_map(|d: &u32| d.to_le_bytes())
        .collect();
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", "thesupersupersigma"),
        ("FileDescription", description),
        ("FileVersion", &version),
        ("InternalName", file.trim_end_matches(".exe")),
        ("LegalCopyright", "thesupersupersigma, GPL-3.0-or-later"),
        ("OriginalFilename", file),
        ("ProductName", "rustyAC"),
        ("ProductVersion", &version),
    ]
    .iter()
    .map(|(k, v)| node(k, &wide(v), true, &[]))
    .collect();
    // language 0x0409 (US English), code page 1200 (Unicode)
    let table = node("040904B0", &[], true, &strings);
    let translation = node("Translation", &[0x09, 0x04, 0xb0, 0x04], false, &[]);
    let data = node(
        "VS_VERSION_INFO",
        &fixed,
        false,
        &[node("StringFileInfo", &[], true, &[table]), node("VarFileInfo", &[], true, &[translation])],
    );
    // a .res file: an empty first entry, then RT_VERSION (16) #1, moveable|pure, US English
    let mut res = Vec::new();
    for (size, kind, flags, lang) in [(0u32, 0u16, 0u16, 0u16), (data.len() as u32, 16, 0x30, 0x409)] {
        res.extend(size.to_le_bytes());
        res.extend(0x20u32.to_le_bytes());
        res.extend([0xff, 0xff]);
        res.extend(kind.to_le_bytes());
        res.extend([0xff, 0xff]);
        res.extend(u16::from(kind != 0).to_le_bytes());
        res.extend(0u32.to_le_bytes());
        res.extend(flags.to_le_bytes());
        res.extend(lang.to_le_bytes());
        res.extend([0; 8]);
    }
    res.extend(&data);
    pad(&mut res);
    let out = std::path::Path::new(&var("OUT_DIR")).join("version.res");
    std::fs::write(&out, res).expect("writing version.res");
    println!("cargo:rustc-link-arg-bin={bin}={}", out.display());
}

/// One VERSIONINFO block: length, value length (WCHARs for text), type, key, value, children,
/// each part on a 4-byte boundary.
fn node(key: &str, value: &[u8], text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0; 6];
    b.extend(wide(key));
    pad(&mut b);
    b.extend(value);
    for c in children {
        pad(&mut b);
        b.extend(c);
    }
    let value_len = if text { value.len() / 2 } else { value.len() };
    let len = b.len() as u16;
    b[0..2].copy_from_slice(&len.to_le_bytes());
    b[2..4].copy_from_slice(&(value_len as u16).to_le_bytes());
    b[4..6].copy_from_slice(&(text as u16).to_le_bytes());
    b
}

fn wide(s: &str) -> Vec<u8> {
    s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
}

fn pad(b: &mut Vec<u8>) {
    b.resize(b.len().next_multiple_of(4), 0);
}
