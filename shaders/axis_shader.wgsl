// shaders/axis_shader.wgsl

// Uniforms for the camera view-projection matrix
@group(0) @binding(0)
var<uniform> camera: mat4x4<f32>;

// Uniforms for the axis model matrix (to position it at the cloud center)
@group(1) @binding(0)
var<uniform> model: mat4x4<f32>;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>, // Using vec3 for color
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera * model * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0); // Return color with full alpha
}