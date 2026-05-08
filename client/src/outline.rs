use bevy::{
    core_pipeline::{
        core_3d::graph::{Core3d, Node3d},
        prepass::ViewPrepassTextures,
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
            CachedRenderPipelineId, PipelineCache, Sampler, ShaderStages, ShaderType,
            TextureSampleType, binding_types::*,
        },
        renderer::RenderContext,
        view::ViewTarget,
    },
};

use crate::fullscreen_post_process::{
    draw_fullscreen_post_process, init_fullscreen_post_process,
};

/// Add to a camera entity to enable screen-space edge outlines.
#[derive(Component, Clone, Copy, ShaderType, ExtractComponent)]
pub struct OutlineSettings {
    /// Edge detection threshold — lower = more edges. Good range: 0.02–0.15.
    pub threshold: f32,
    /// Outline color and opacity (RGBA).
    pub color: Vec4,
}

impl Default for OutlineSettings {
    fn default() -> Self {
        Self { threshold: 0.05, color: Vec4::new(1.0, 1.0, 1.0, 0.8) }
    }
}

pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<OutlineSettings>::default(),
            UniformComponentPlugin::<OutlineSettings>::default(),
        ));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_render_graph_node::<ViewNodeRunner<OutlineNode>>(Core3d, OutlineLabel)
            .add_render_graph_edges(
                Core3d,
                (Node3d::Smaa, OutlineLabel, Node3d::EndMainPassPostProcessing),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<OutlinePipeline>();
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct OutlineLabel;

#[derive(Resource)]
struct OutlinePipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

impl FromWorld for OutlinePipeline {
    fn from_world(world: &mut World) -> Self {
        let entries = BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }), // screen color
                sampler(bevy::render::render_resource::SamplerBindingType::Filtering),
                texture_depth_2d(),                                        // depth
                texture_2d(TextureSampleType::Float { filterable: true }), // normals (Rgb10a2Unorm is filterable)
                uniform_buffer::<OutlineSettings>(true),
            ),
        );
        let layout = BindGroupLayoutDescriptor::new("outline_layout", &entries);
        let (sampler, pipeline_id) =
            init_fullscreen_post_process(world, &layout, "shaders/outline.wgsl", "outline_pipeline");
        Self { layout, sampler, pipeline_id }
    }
}

#[derive(Default)]
struct OutlineNode;

impl ViewNode for OutlineNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static DynamicUniformIndex<OutlineSettings>,
        &'static ViewPrepassTextures,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (view_target, settings_index, prepass_textures): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let pipeline = world.resource::<OutlinePipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let settings_uniforms = world.resource::<ComponentUniforms<OutlineSettings>>();

        let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline.pipeline_id) else {
            return Ok(());
        };
        let Some(settings_binding) = settings_uniforms.uniforms().binding() else {
            return Ok(());
        };
        let Some(depth) = prepass_textures.depth.as_ref() else {
            return Ok(());
        };
        let Some(normals) = prepass_textures.normal.as_ref() else {
            return Ok(());
        };

        let post_process = view_target.post_process_write();
        let bind_group = render_context.render_device().create_bind_group(
            Some("outline_bind_group"),
            &pipeline_cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                post_process.source,
                &pipeline.sampler,
                &depth.texture.default_view,
                &normals.texture.default_view,
                settings_binding.clone(),
            )),
        );

        draw_fullscreen_post_process(
            render_context,
            render_pipeline,
            &bind_group,
            &[settings_index.index()],
            post_process.destination,
            "outline_pass",
        );
        Ok(())
    }
}
