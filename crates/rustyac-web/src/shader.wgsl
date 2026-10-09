// SPDX-License-Identifier: GPL-3.0-or-later

// The browser picture's shaders: the desktop debug view's (`rustyac-game/src/render/mod.rs`,
// HLSL) in WGSL, line for line. One sun, ambient light, distance fog; a kn5 mesh's colour as
// AC's pixel shader of that material mixes it (plain, multilayer ground, grass, detail).
//
// The matrices are uploaded as the row-major arrays the rest of rustyAC uses (row vectors:
// `v * M`). WGSL reads the same sixteen numbers as columns, which is the transpose, so
// `M * v` here is `v * M` there.

struct Draw {
    world: mat4x4<f32>,
    // World x the camera's view matrix, worked out in double precision
    world_view: mat4x4<f32>,
    proj: mat4x4<f32>,
    color: vec4<f32>,
    // xyz: the direction the light travels, w: ambient share
    light: vec4<f32>,
    // xyz: the camera's position, w: fog density per metre
    camera: vec4<f32>,
    // rgb: the colour of the horizon
    fog: vec4<f32>,
    // x: pixels with less alpha are not drawn, y: 1 = lit from both sides,
    // z: repeats of the detail texture (0 = none)
    params: vec4<f32>,
    // the repeats of the detail textures R (xy) and G (zw)
    mult_rg: vec4<f32>,
    // ... B (xy) and A (zw)
    mult_ba: vec4<f32>,
    // x: 0 plain, 1 multilayer by world position, 2 multilayer by uv, 3 grass;
    // y: magicMult (grass: gain); z, w: ksAmbient, ksDiffuse (z < 0: none)
    layer: vec4<f32>,
    // x: the diffuse texture's uv factor, y: alpha factor
    layer2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Draw;

@group(1) @binding(0) var t_diffuse: texture_2d<f32>;
@group(1) @binding(1) var t_detail: texture_2d<f32>;
// txMask (grass: txVariation)
@group(1) @binding(2) var t_mask: texture_2d<f32>;
@group(1) @binding(3) var t_detail_r: texture_2d<f32>;
@group(1) @binding(4) var t_detail_g: texture_2d<f32>;
@group(1) @binding(5) var t_detail_b: texture_2d<f32>;
@group(1) @binding(6) var t_detail_a: texture_2d<f32>;
@group(1) @binding(7) var s_linear: sampler;

struct MeshOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world: vec3<f32>,
};

struct ModelOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

fn fogged(c: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let d = length(world - u.camera.xyz);
    return mix(c, u.fog.rgb, 1.0 - exp(-d * u.camera.w));
}

@vertex
fn vs_mesh(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>) -> MeshOut {
    var o: MeshOut;
    let p = vec4<f32>(pos, 1.0);
    o.pos = u.proj * (u.world_view * p);
    o.normal = (u.world * vec4<f32>(normal, 0.0)).xyz;
    o.world = (u.world * p).xyz;
    return o;
}

@fragment
fn fs_mesh(i: MeshOut) -> @location(0) vec4<f32> {
    let n = normalize(i.normal);
    let lit = saturate(dot(n, -u.light.xyz));
    let c = u.color.rgb * (u.light.w + (1.0 - u.light.w) * lit);
    return vec4<f32>(fogged(c, i.world), u.color.a);
}

@vertex
fn vs_model(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> ModelOut {
    var o: ModelOut;
    let p = vec4<f32>(pos, 1.0);
    o.pos = u.proj * (u.world_view * p);
    o.normal = (u.world * vec4<f32>(normal, 0.0)).xyz;
    o.world = (u.world * p).xyz;
    o.uv = uv;
    return o;
}

// a kn5 mesh: its colour as AC's pixel shader of that material mixes it, one sun, ambient light
@fragment
fn fs_model(i: ModelOut) -> @location(0) vec4<f32> {
    var t = textureSample(t_diffuse, s_linear, i.uv * u.layer2.x) * u.color;
    // every texture is sampled where the derivatives are defined (outside the branches)
    let by_world = u.layer.x == 1.0;
    let p = select(i.uv, i.world.xz, by_world);
    let m = textureSample(t_mask, s_linear, select(i.uv, i.world.xz * u.mult_rg.xy, u.layer.x == 3.0));
    let detail_r = textureSample(t_detail_r, s_linear, p * u.mult_rg.xy).rgb;
    let detail_g = textureSample(t_detail_g, s_linear, p * u.mult_rg.zw).rgb;
    let detail_b = textureSample(t_detail_b, s_linear, p * u.mult_ba.xy).rgb;
    let detail_a = textureSample(t_detail_a, s_linear, p * u.mult_ba.zw).rgb;
    let detail = textureSample(t_detail, s_linear, i.uv * u.params.z).rgb;
    if (u.layer.x == 1.0 || u.layer.x == 2.0) {
        // ksMultilayer*: the diffuse texture only shades; the colour is four tiled detail
        // textures weighted by the mask's channels (the weights are not normalised)
        let d = detail_g * m.g + detail_r * m.r + detail_b * m.b + detail_a * m.a;
        t = vec4<f32>(t.rgb * d * u.layer.y, 1.0);
    } else if (u.layer.x == 3.0) {
        // ksGrass: the blades' colour varied over the ground
        t = vec4<f32>(t.rgb + t.rgb * (m.rgb - 0.5) * u.layer.y, t.a);
    }
    if (u.params.z > 0.0) {
        // AC's detail texture: the colour where the diffuse texture's alpha is 0
        t = vec4<f32>(t.rgb * mix(detail, vec3<f32>(1.0, 1.0, 1.0), t.a), 1.0);
    }
    t.a = t.a * u.layer2.y;
    if (t.a - u.params.x < 0.0) {
        discard;
    }
    let n = normalize(i.normal);
    let d = dot(n, -u.light.xyz);
    let lit = mix(saturate(d), abs(d) * 0.5 + 0.5, u.params.y);
    var c: vec3<f32>;
    if (u.layer.z >= 0.0) {
        // the shape of AC's light: the sky's share by how far the surface faces up
        // (ksAmbient), the sun's by the angle (ksDiffuse); the two strengths stand in for
        // the weather's colours
        c = saturate(t.rgb * (1.2 * u.layer.z * saturate(0.75 + 0.25 * n.y) + 2.0 * u.layer.w * lit));
    } else {
        c = t.rgb * (u.light.w + (1.0 - u.light.w) * lit);
    }
    return vec4<f32>(fogged(c, i.world), t.a);
}
