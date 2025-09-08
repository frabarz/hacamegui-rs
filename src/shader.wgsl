struct CameraUniform {
    inv_proj: mat4x4<f32>,
    inv_view: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> cam: CameraUniform;

@group(0) @binding(1)
var panorama: texture_2d<f32>;

@group(0) @binding(2)
var samp: sampler;

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    var out: VertexOutput;

    // fullscreen triangle
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    out.clip = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = out.clip.xy * 0.5 + vec2(0.5);

    return out;
}

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    // reconstructing view ray from screen UV
    let ndc = vec4<f32>(uv * 2.0 - vec2<f32>(1.0), 1.0, 1.0);
    let view_ray = (cam.inv_proj * ndc).xyz;
    let dir = normalize((cam.inv_view * vec4<f32>(view_ray, 0.0)).xyz);

    // converting direction to equirectangular UV
    let lon = atan2(dir.x, dir.z);
    let lat = asin(dir.y);
    let tex_u = lon / (2.0 * 3.14159265) + 0.5;
    let tex_v = 0.5 - lat / 3.14159265;

    return textureSample(panorama, samp, vec2(tex_u, tex_v));
}
