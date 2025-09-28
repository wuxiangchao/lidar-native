// Uniform buffer for camera data.
// This structure must match the `CameraUniform` struct in `renderer.rs`
// after the alignment fix.
struct Camera {
    view_proj: mat4x4<f32>,
    point_size: f32,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

// Input vertex data from the vertex buffer.
// This matches the `Point` struct layout in `common.rs`.
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

// Data passed from the vertex shader to the fragment shader.
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(
    model: VertexInput,
    @builtin(vertex_index) vertex_index: u32,
) -> VertexOutput {

    // Based on the vertex index (0 to 3), determine which corner
    // of the square billboard to generate.
    var corner_offset: vec2<f32>;
    switch vertex_index {
        case 0u: {
            corner_offset = vec2<f32>(-1.0, 1.0); // Top-left
        }
        case 1u: {
            corner_offset = vec2<f32>(-1.0, -1.0); // Bottom-left
        }
        case 2u: {
            corner_offset = vec2<f32>(1.0, 1.0); // Top-right
        }
        default: { // case 3u
            corner_offset = vec2<f32>(1.0, -1.0); // Bottom-right
        }
    }

    var out: VertexOutput;

    out.clip_position = camera.view_proj * vec4<f32>(model.position, 1.0);

    let offset_in_clip_space = corner_offset * camera.point_size * out.clip_position.w * 0.05;


    out.clip_position.x += offset_in_clip_space.x;
    out.clip_position.y += offset_in_clip_space.y;

    // Pass the original color to the fragment shader.
    out.color = model.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Simply return the color passed from the vertex shader.
    return in.color;
}