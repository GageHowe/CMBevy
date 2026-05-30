use bevy::{
    asset::{AssetServer, Assets},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        gpu_readback::{Readback, ReadbackComplete},
        render_asset::RenderAssets,
        render_graph::{RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner},
        render_resource::{
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            CachedComputePipelineId, ComputePassDescriptor, ComputePipelineDescriptor,
            PipelineCache, ShaderStages, binding_types::*,
        },
        renderer::RenderContext,
        storage::{GpuShaderStorageBuffer, ShaderStorageBuffer},
        view::{Hdr, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

#[derive(Resource, Default)]
pub struct AutoExposureCorrection(pub Option<f32>);

#[derive(Resource, ExtractResource, Clone)]
struct AutoExposureCorrectionBuffer(Handle<ShaderStorageBuffer>);

pub struct AutoExposureDebugPlugin;

impl Plugin for AutoExposureDebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AutoExposureCorrection>()
            .add_systems(Startup, setup_auto_exposure_debug)
            .add_plugins(ExtractResourcePlugin::<AutoExposureCorrectionBuffer>::default());

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .add_systems(RenderStartup, init_auto_exposure_debug_pipeline)
            .add_render_graph_node::<ViewNodeRunner<AutoExposureDebugNode>>(
                bevy::core_pipeline::core_3d::graph::Core3d,
                AutoExposureDebugLabel,
            )
            .add_render_graph_edges(
                bevy::core_pipeline::core_3d::graph::Core3d,
                (
                    bevy::core_pipeline::core_3d::graph::Node3d::Tonemapping,
                    AutoExposureDebugLabel,
                    bevy::core_pipeline::core_3d::graph::Node3d::EndMainPassPostProcessing,
                ),
            );
    }
}

fn setup_auto_exposure_debug(
    mut commands: Commands,
    mut buffers: ResMut<Assets<ShaderStorageBuffer>>,
) {
    let mut buffer = ShaderStorageBuffer::from(0.0f32);
    buffer.buffer_description.usage |= bevy::render::render_resource::BufferUsages::COPY_SRC;
    let buffer = buffers.add(buffer);
    commands.insert_resource(AutoExposureCorrectionBuffer(buffer.clone()));
    commands
        .spawn(Readback::buffer(buffer))
        .observe(update_auto_exposure_correction);
}

fn update_auto_exposure_correction(
    event: On<ReadbackComplete>,
    mut correction: ResMut<AutoExposureCorrection>,
) {
    correction.0 = Some(event.to_shader_type::<f32>());
}

#[derive(Resource)]
struct AutoExposureDebugPipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline_id: CachedComputePipelineId,
}

fn init_auto_exposure_debug_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "auto_exposure_debug_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<ViewUniform>(true),
                storage_buffer::<f32>(false),
            ),
        ),
    );
    let pipeline_id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("auto_exposure_debug_pipeline".into()),
        layout: vec![layout.clone()],
        shader: asset_server.load("shaders/auto_exposure_debug.wgsl"),
        ..default()
    });
    commands.insert_resource(AutoExposureDebugPipeline {
        layout,
        pipeline_id,
    });
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct AutoExposureDebugLabel;

#[derive(Default)]
struct AutoExposureDebugNode;

impl ViewNode for AutoExposureDebugNode {
    type ViewQuery = (
        &'static ViewUniformOffset,
        &'static bevy::post_process::auto_exposure::AutoExposure,
        &'static Hdr,
    );

    fn run(
        &self,
        _graph: &mut bevy::render::render_graph::RenderGraphContext,
        render_context: &mut RenderContext,
        (view_offset, _auto_exposure, _hdr): bevy::ecs::query::QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), bevy::render::render_graph::NodeRunError> {
        let pipeline = world.resource::<AutoExposureDebugPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let view_uniforms = world.resource::<ViewUniforms>();
        let output_handle = world.resource::<AutoExposureCorrectionBuffer>();
        let output_buffers = world.resource::<RenderAssets<GpuShaderStorageBuffer>>();

        let Some(compute_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.pipeline_id)
        else {
            return Ok(());
        };
        let Some(view_binding) = view_uniforms.uniforms.binding() else {
            return Ok(());
        };
        let Some(output_buffer) = output_buffers.get(&output_handle.0) else {
            return Ok(());
        };

        let bind_group = render_context.render_device().create_bind_group(
            Some("auto_exposure_debug_bind_group"),
            &pipeline_cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                view_binding,
                output_buffer.buffer.as_entire_buffer_binding(),
            )),
        );

        let mut pass =
            render_context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("auto_exposure_debug_pass"),
                    ..default()
                });
        pass.set_bind_group(0, &bind_group, &[view_offset.offset]);
        pass.set_pipeline(compute_pipeline);
        pass.dispatch_workgroups(1, 1, 1);

        Ok(())
    }
}
