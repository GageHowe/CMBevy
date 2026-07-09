use bevy::{
    core_pipeline::FullscreenShader,
    prelude::*,
    render::{
        render_resource::{
            BindGroup, BindGroupLayoutDescriptor, CachedRenderPipelineId, ColorTargetState,
            ColorWrites, FragmentState, MultisampleState, Operations, PipelineCache,
            PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerDescriptor, TextureFormat, TextureView,
        },
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
    },
};

pub(crate) fn init_fullscreen_post_process(
    world: &mut World,
    layout: &BindGroupLayoutDescriptor,
    shader_path: &'static str,
    pipeline_label: &'static str,
) -> (Sampler, CachedRenderPipelineId, CachedRenderPipelineId) {
    let render_device = world.resource::<RenderDevice>();
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());
    let shader = world.load_asset(shader_path);
    let fullscreen = world.resource::<FullscreenShader>().clone();
    let mut descriptor = RenderPipelineDescriptor {
        label: Some(pipeline_label.into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader,
            shader_defs: vec![],
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::bevy_default(),
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        push_constant_ranges: vec![],
        zero_initialize_workgroup_memory: false,
    };
    let pipeline_cache = world.resource::<PipelineCache>();
    let pipeline_id = pipeline_cache.queue_render_pipeline(descriptor.clone());
    if let Some(target) = descriptor
        .fragment
        .as_mut()
        .and_then(|fragment| fragment.targets.first_mut())
        .and_then(Option::as_mut)
    {
        target.format = ViewTarget::TEXTURE_FORMAT_HDR;
    }
    let pipeline_id_hdr = pipeline_cache.queue_render_pipeline(descriptor);
    (sampler, pipeline_id, pipeline_id_hdr)
}

pub(crate) fn draw_fullscreen_post_process(
    render_context: &mut RenderContext,
    render_pipeline: &RenderPipeline,
    bind_group: &BindGroup,
    dynamic_indices: &[u32],
    destination: &TextureView,
    pass_label: &'static str,
) {
    let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some(pass_label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    render_pass.set_render_pipeline(render_pipeline);
    render_pass.set_bind_group(0, bind_group, dynamic_indices);
    render_pass.draw(0..3, 0..1);
}
