#import bevy_render::view::View

@group(0) @binding(0) var<storage, read> view: View;
@group(0) @binding(1) var<storage, read_write> output: f32;

@compute @workgroup_size(1, 1, 1)
fn main() {
    output = view.color_grading.exposure;
}
