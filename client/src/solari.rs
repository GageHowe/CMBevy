use bevy::{
    asset::AssetId,
    mesh::{Indices, MeshVertexAttributeId, VertexAttributeValues},
    pbr::MeshMaterial3d,
    platform::collections::HashMap,
    prelude::*,
    solari::prelude::{RaytracingMesh3d, SolariPlugins},
};

pub struct SolariTogglePlugin;

impl Plugin for SolariTogglePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SolariProxyMeshes>()
            .add_plugins(SolariPlugins)
            .add_systems(PostUpdate, tag_solari_meshes);
    }
}

#[derive(Resource, Default)]
struct SolariProxyMeshes(HashMap<AssetId<Mesh>, Handle<Mesh>>);

fn tag_solari_meshes(
    mut commands: Commands,
    mut meshes_assets: ResMut<Assets<Mesh>>,
    mut proxy_meshes: ResMut<SolariProxyMeshes>,
    meshes: Query<
        (Entity, &Mesh3d),
        (
            With<MeshMaterial3d<StandardMaterial>>,
            Without<RaytracingMesh3d>,
        ),
    >,
) {
    for (entity, mesh) in &meshes {
        let source_id = mesh.id();
        if let Some(proxy) = proxy_meshes.0.get(&source_id) {
            commands.entity(entity).insert(RaytracingMesh3d(proxy.clone()));
            continue;
        }

        let Some(source_mesh) = meshes_assets.get(&mesh.0) else {
            continue;
        };
        let Some(proxy_mesh) = make_solari_proxy_mesh(source_mesh) else {
            continue;
        };

        let proxy = meshes_assets.add(proxy_mesh);
        proxy_meshes.0.insert(source_id, proxy.clone());
        commands.entity(entity).insert(RaytracingMesh3d(proxy));
    }
}

fn make_solari_proxy_mesh(source: &Mesh) -> Option<Mesh> {
    if source.primitive_topology() != bevy::render::render_resource::PrimitiveTopology::TriangleList
    {
        return None;
    }

    let positions = clone_attribute(source, Mesh::ATTRIBUTE_POSITION.id)?;
    let normals = clone_attribute(source, Mesh::ATTRIBUTE_NORMAL.id)?;
    let uvs = clone_attribute(source, Mesh::ATTRIBUTE_UV_0.id)?;
    let indices = match source.indices()? {
        Indices::U16(values) => Indices::U32(values.iter().map(|&i| i as u32).collect()),
        Indices::U32(values) => Indices::U32(values.clone()),
    };

    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        source.asset_usage,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(indices);
    if let Some(tangents) = clone_attribute(source, Mesh::ATTRIBUTE_TANGENT.id) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    } else if mesh.generate_tangents().is_err() {
        return None;
    }
    mesh.enable_raytracing = true;
    Some(mesh)
}

fn clone_attribute(source: &Mesh, attribute: MeshVertexAttributeId) -> Option<VertexAttributeValues> {
    source.attribute(attribute).cloned()
}
