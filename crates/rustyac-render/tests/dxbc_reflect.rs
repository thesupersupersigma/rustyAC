// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyac-dxbc's reflection of every shader in AC's `system/shaders/win` against
//! `D3DReflect`'s: the constant buffers (name, size, slot), their variables (name, offset,
//! size) and the bound resources (name, type, register), as `KGLShader::reflectVars` reads them.

use std::ffi::c_void;

use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;
use windows::Win32::Graphics::Direct3D11::*;

unsafe fn text(p: PCSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    p.as_bytes().iter().map(|b| *b as char).collect()
}

/// (buffers as (name, size, slot, [(var, offset, size)]), resources as (name, type, slot))
type Seen = (Vec<(String, u32, u32, Vec<(String, u32, u32)>)>, Vec<(String, u32, u32)>);

unsafe fn d3d(blob: &[u8]) -> Seen {
    let mut r: Option<ID3D11ShaderReflection> = None;
    D3DReflect(blob.as_ptr() as *const c_void, blob.len(), &ID3D11ShaderReflection::IID, &mut r as *mut _ as *mut *mut c_void).unwrap();
    let r = r.unwrap();
    let mut sd = D3D11_SHADER_DESC::default();
    r.GetDesc(&mut sd).unwrap();
    let mut buffers = Vec::new();
    for i in 0..sd.ConstantBuffers {
        let cb = r.GetConstantBufferByIndex(i).unwrap();
        let mut cbd = D3D11_SHADER_BUFFER_DESC::default();
        cb.GetDesc(&mut cbd).unwrap();
        let mut bd = D3D11_SHADER_INPUT_BIND_DESC::default();
        let _ = r.GetResourceBindingDescByName(cbd.Name, &mut bd);
        let mut vars = Vec::new();
        for j in 0..cbd.Variables {
            let mut vd = D3D11_SHADER_VARIABLE_DESC::default();
            cb.GetVariableByIndex(j).unwrap().GetDesc(&mut vd).unwrap();
            vars.push((text(vd.Name), vd.StartOffset, vd.Size));
        }
        buffers.push((text(cbd.Name), cbd.Size, bd.BindPoint, vars));
    }
    let mut resources = Vec::new();
    for k in 0..sd.BoundResources {
        let mut bd = D3D11_SHADER_INPUT_BIND_DESC::default();
        r.GetResourceBindingDesc(k, &mut bd).unwrap();
        resources.push((text(bd.Name), bd.Type.0 as u32, bd.BindPoint));
    }
    (buffers, resources)
}

fn ours(blob: &[u8]) -> Seen {
    let r = rustyac_dxbc::parse(blob).unwrap().reflection;
    let buffers = r
        .constant_buffers
        .iter()
        .map(|c| (c.name.clone(), c.size, r.binding(&c.name).map_or(0, |b| b.bind_point), c.variables.iter().map(|v| (v.name.clone(), v.offset, v.size)).collect()))
        .collect();
    let resources = r.bindings.iter().map(|b| (b.name.clone(), b.kind, b.bind_point)).collect();
    (buffers, resources)
}

#[test]
fn reflection_matches_d3dreflect() {
    let Some(game) = rustyac_content::install::ac_root() else {
        eprintln!("NOT TESTED: no Assetto Corsa install");
        return;
    };
    let folder = game.join("system/shaders/win");
    let mut n = 0;
    for e in std::fs::read_dir(&folder).unwrap() {
        let path = e.unwrap().path();
        if path.extension().and_then(|x| x.to_str()) != Some("fxo") {
            continue;
        }
        let blob = std::fs::read(&path).unwrap();
        assert_eq!(ours(&blob), unsafe { d3d(&blob) }, "{}", path.display());
        n += 1;
    }
    eprintln!("{n} shaders: the same reflection as D3DReflect");
    assert!(n > 0);
}
