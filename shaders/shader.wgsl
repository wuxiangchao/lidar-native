struct CameraUniform {
    view_proj: mat4x4<f32>,
    point_size: f32,
};

@group(0) @binding(0)
var<uniform> u_camera: CameraUniform;

struct VertexInput {
    @location(0) instance_position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    instance: VertexInput,
) -> VertexOutput {
    var offset: vec2<f32>;
    switch vertex_index {
        case 0u: {
            offset = vec2<f32>(-0.5, -0.5);
        }
        case 1u: {
            offset = vec2<f32>(0.5, -0.5);
        }
        case 2u: {
            offset = vec2<f32>(-0.5, 0.5);
        }
        default: { // case 3u
            offset = vec2<f32>(0.5, 0.5);
        }
    }

    let world_position = vec4<f32>(instance.instance_position, 1.0);
    var clip_position = u_camera.view_proj * world_position;

    // Use a small constant to scale the point size to be reasonable in screen space
    let screen_space_offset = offset * u_camera.point_size * clip_position.w * 0.05;

    // FIX: Assign to each component individually
    clip_position.x += screen_space_offset.x;
    clip_position.y += screen_space_offset.y;

    var out: VertexOutput;
    out.clip_position = clip_position;
    return out;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 1.0, 0.5, 1.0);
}