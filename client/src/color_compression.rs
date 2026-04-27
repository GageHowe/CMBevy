use bevy::{
    core_pipeline::{
        FullscreenShader,
        core_3d::graph::{Core3d, Node3d},
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        RenderApp,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, MultisampleState,
            Operations, PipelineCache, PrimitiveState, RenderPassColorAttachment,
            RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerDescriptor,
            ShaderStages, ShaderType, TextureSampleType, binding_types::*,
        },
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
    },
};

use crate::outline::OutlineLabel;

#[derive(Component, Clone, Copy, ShaderType, ExtractComponent)]
pub struct ColorCompressionSettings {
    pub color_steps: f32,
    pub dither_strength: f32,
}

impl Default for ColorCompressionSettings {
    fn default() -> Self {
        Self { color_steps: 24.0, dither_strength: 0.75 }
    }
}

pub struct ColorCompressionPlugin;

impl Plugin for ColorCompressionPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<ColorCompressionSettings>::default(),
            UniformComponentPlugin::<ColorCompressionSettings>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_render_graph_node::<ViewNodeRunner<ColorCompressionNode>>(
                Core3d,
                ColorCompressionLabel,
            )
            .add_render_graph_edges(
                Core3d,
                (Node3d::Smaa, ColorCompressionLabel, OutlineLabel),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<ColorCompressionPipeline>();
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct ColorCompressionLabel;

#[derive(Resource)]
struct ColorCompressionPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

impl FromWorld for ColorCompressionPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let entries = BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(bevy::render::render_resource::SamplerBindingType::Filtering),
                uniform_buffer::<ColorCompressionSettings>(true),
            ),
        );
        let layout = BindGroupLayoutDescriptor::new("color_compression_layout", &entries);
        let sampler = render_device.create_sampler(&SamplerDescriptor::default());
        let shader = world.load_asset("shaders/color_compression.wgsl");
        let fullscreen = world.resource::<FullscreenShader>().clone();
        let pipeline_id =
            world.resource::<PipelineCache>().queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("color_compression_pipeline".into()),
                layout: vec![layout.clone()],
                vertex: fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader,
                    shader_defs: vec![],
                    targets: vec![Some(ColorTargetState {
                        format: ViewTarget::TEXTURE_FORMAT_HDR,
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
            });
        Self { layout, sampler, pipeline_id }
    }
}

#[derive(Default)]
struct ColorCompressionNode;

impl ViewNode for ColorCompressionNode {
    type ViewQuery = (&'static ViewTarget, &'static DynamicUniformIndex<ColorCompressionSettings>);

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (view_target, settings_index): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let pipeline = world.resource::<ColorCompressionPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let settings_uniforms = world.resource::<ComponentUniforms<ColorCompressionSettings>>();

        let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline.pipeline_id) else {
            return Ok(());
        };
        let Some(settings_binding) = settings_uniforms.uniforms().binding() else {
            return Ok(());
        };

        let post_process = view_target.post_process_write();
        let bind_group = render_context.render_device().create_bind_group(
            Some("color_compression_bind_group"),
            &pipeline_cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                post_process.source,
                &pipeline.sampler,
                settings_binding.clone(),
            )),
        );

        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("color_compression_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_render_pipeline(render_pipeline);
        render_pass.set_bind_group(0, &bind_group, &[settings_index.index()]);
        render_pass.draw(0..3, 0..1);
        Ok(())
    }
}
