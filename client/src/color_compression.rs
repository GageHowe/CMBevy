use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
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
            CachedRenderPipelineId, PipelineCache, Sampler, ShaderStages, ShaderType,
            TextureSampleType, binding_types::*,
        },
        renderer::RenderContext,
        view::ViewTarget,
    },
};

use crate::fullscreen_post_process::{draw_fullscreen_post_process, init_fullscreen_post_process};

#[derive(Component, Clone, Copy, ShaderType, ExtractComponent)]
pub struct ColorCompressionSettings {
    pub color_steps: f32,
    pub dither_strength: f32,
}

impl Default for ColorCompressionSettings {
    fn default() -> Self {
        Self {
            color_steps: 24.0,
            dither_strength: 0.75,
        }
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
                (
                    Node3d::Smaa,
                    ColorCompressionLabel,
                    Node3d::EndMainPassPostProcessing,
                ),
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
    pipeline_id_hdr: CachedRenderPipelineId,
}

impl FromWorld for ColorCompressionPipeline {
    fn from_world(world: &mut World) -> Self {
        let entries = BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(bevy::render::render_resource::SamplerBindingType::Filtering),
                uniform_buffer::<ColorCompressionSettings>(true),
            ),
        );
        let layout = BindGroupLayoutDescriptor::new("color_compression_layout", &entries);
        let (sampler, pipeline_id, pipeline_id_hdr) = init_fullscreen_post_process(
            world,
            &layout,
            "shaders/color_compression.wgsl",
            "color_compression_pipeline",
        );
        Self {
            layout,
            sampler,
            pipeline_id,
            pipeline_id_hdr,
        }
    }
}

#[derive(Default)]
struct ColorCompressionNode;
impl ViewNode for ColorCompressionNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static DynamicUniformIndex<ColorCompressionSettings>,
    );

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

        let pipeline_id = if view_target.is_hdr() {
            pipeline.pipeline_id_hdr
        } else {
            pipeline.pipeline_id
        };
        let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline_id) else {
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

        draw_fullscreen_post_process(
            render_context,
            render_pipeline,
            &bind_group,
            &[settings_index.index()],
            post_process.destination,
            "color_compression_pass",
        );
        Ok(())
    }
}
