use bevy::{
    core_pipeline::{
        FullscreenShader,
        prepass::ViewPrepassTextures,
        schedule::{Core3d, Core3dSystems},
        tonemapping::tonemapping,
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_resource::{
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, Operations,
            PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            ShaderType, SpecializedRenderPipeline, SpecializedRenderPipelines, TextureFormat,
            TextureSampleType, binding_types::*,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::{ExtractedView, ViewTarget},
    },
    shader::Shader,
};

/// Add to a camera entity to enable screen-space edge outlines.
#[derive(Component, Clone, Copy, ShaderType, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct OutlineSettings {
    /// Edge detection threshold — lower = more edges. Good range: 0.02–0.15.
    pub threshold: f32,
    /// Outline color and opacity (RGBA).
    pub color: Vec4,
}

impl Default for OutlineSettings {
    fn default() -> Self {
        Self {
            threshold: 0.05,
            color: Vec4::new(1.0, 1.0, 1.0, 0.8),
        }
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
            .init_resource::<SpecializedRenderPipelines<OutlinePipeline>>()
            .add_systems(RenderStartup, init_outline_pipeline)
            .add_systems(
                Render,
                prepare_outline_pipelines.in_set(RenderSystems::Prepare),
            )
            .add_systems(
                Core3d,
                outline
                    .after(bevy::anti_alias::smaa::smaa)
                    .after(tonemapping)
                    .in_set(Core3dSystems::PostProcess),
            );
    }
}

#[derive(Component)]
struct OutlinePipelineId(CachedRenderPipelineId);

#[derive(Resource)]
struct OutlinePipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    shader: Handle<Shader>,
    fullscreen: FullscreenShader,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct OutlinePipelineKey {
    target_format: TextureFormat,
}

impl SpecializedRenderPipeline for OutlinePipeline {
    type Key = OutlinePipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("outline_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format: key.target_format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            zero_initialize_workgroup_memory: false,
            ..default()
        }
    }
}

fn init_outline_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    render_device: Res<RenderDevice>,
    fullscreen: Res<FullscreenShader>,
) {
    commands.insert_resource(OutlinePipeline {
        layout: BindGroupLayoutDescriptor::new(
            "outline_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }), // screen color
                    sampler(SamplerBindingType::Filtering),
                    texture_depth_2d(), // depth
                    texture_2d(TextureSampleType::Float { filterable: true }), // normals (Rgb10a2Unorm is filterable)
                    uniform_buffer::<OutlineSettings>(true),
                ),
            ),
        ),
        sampler: render_device.create_sampler(&SamplerDescriptor::default()),
        shader: asset_server.load("shaders/outline.wgsl"),
        fullscreen: fullscreen.clone(),
    });
}

fn prepare_outline_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<OutlinePipeline>,
    mut specialized: ResMut<SpecializedRenderPipelines<OutlinePipeline>>,
    views: Query<(Entity, &ExtractedView), (With<ExtractedCamera>, With<OutlineSettings>)>,
) {
    for (entity, view) in &views {
        let pipeline_id = specialized.specialize(
            &pipeline_cache,
            &pipeline,
            OutlinePipelineKey {
                target_format: view.target_format,
            },
        );
        commands
            .entity(entity)
            .insert(OutlinePipelineId(pipeline_id));
    }
}

fn outline(
    view: ViewQuery<(
        &ViewTarget,
        &DynamicUniformIndex<OutlineSettings>,
        &ViewPrepassTextures,
        &OutlinePipelineId,
    )>,
    pipeline: Res<OutlinePipeline>,
    pipeline_cache: Res<PipelineCache>,
    settings_uniforms: Res<ComponentUniforms<OutlineSettings>>,
    mut render_context: RenderContext,
) {
    let (view_target, settings_index, prepass_textures, pipeline_id) = view.into_inner();

    let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else {
        return;
    };
    let Some(settings_binding) = settings_uniforms.uniforms().binding() else {
        return;
    };
    let Some(depth) = prepass_textures.depth.as_ref() else {
        return;
    };
    let Some(normals) = prepass_textures.normal.as_ref() else {
        return;
    };

    let post_process = view_target.post_process_write();
    let bind_group = render_context.render_device().create_bind_group(
        "outline_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &pipeline.sampler,
            &depth.texture.default_view,
            &normals.texture.default_view,
            settings_binding.clone(),
        )),
    );

    let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("outline_pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(render_pipeline);
    pass.set_bind_group(0, &bind_group, &[settings_index.index()]);
    pass.draw(0..3, 0..1);
}
