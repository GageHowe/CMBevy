use std::{fmt::Write as FmtWrite, path::PathBuf};

use clap::Parser;
use parry3d::{
    math::Vector,
    transformation::vhacd::{VHACD, VHACDParameters},
};

#[derive(Parser)]
#[command(about = "Convex decompose a GLB mesh using VHACD")]
struct Args {
    /// Input GLB file
    input: PathBuf,
    /// Output OBJ file (default: <input>.hulls.obj)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Max number of convex hulls
    #[arg(long, default_value_t = 16)]
    max_hulls: u32,
    /// Voxel resolution
    #[arg(long, default_value_t = 64)]
    resolution: u32,
    /// Concavity threshold (lower = more precise)
    #[arg(long, default_value_t = 0.01)]
    concavity: f64,
    /// Convex hull downsampling
    #[arg(long, default_value_t = 4)]
    downsampling: u32,
}

const IDENTITY: [[f32; 4]; 4] =
    [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];

// GLTF matrices are column-major: matrix[col][row]
fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            for k in 0..4 {
                out[col][row] += a[k][row] * b[col][k];
            }
        }
    }
    out
}

fn transform_point(m: [[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    let (x, y, z) = (p[0], p[1], p[2]);
    [
        m[0][0] * x + m[1][0] * y + m[2][0] * z + m[3][0],
        m[0][1] * x + m[1][1] * y + m[2][1] * z + m[3][1],
        m[0][2] * x + m[1][2] * y + m[2][2] * z + m[3][2],
    ]
}

fn collect_node(
    node: gltf::Node,
    parent: [[f32; 4]; 4],
    buffers: &[gltf::buffer::Data],
    all_vertices: &mut Vec<Vector>,
    all_indices: &mut Vec<[u32; 3]>,
) {
    let transform = mat4_mul(parent, node.transform().matrix());

    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buf| buffers.get(buf.index()).map(|b| b.0.as_slice()));
            let base = all_vertices.len() as u32;

            let positions: Vec<[f32; 3]> = match reader.read_positions() {
                Some(p) => p.collect(),
                None => continue,
            };
            all_vertices.extend(positions.iter().map(|p| {
                let tp = transform_point(transform, *p);
                Vector::new(tp[0], tp[1], tp[2])
            }));

            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            for tri in indices.chunks_exact(3) {
                all_indices.push([base + tri[0], base + tri[1], base + tri[2]]);
            }
        }
    }

    for child in node.children() {
        collect_node(child, transform, buffers, all_vertices, all_indices);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let output = args.output.unwrap_or_else(|| {
        let mut p = args.input.clone();
        let stem = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        p.set_file_name(format!("{stem}.hulls.obj"));
        p
    });

    let (gltf, buffers, _) = gltf::import(&args.input)?;

    let mut all_vertices: Vec<Vector> = Vec::new();
    let mut all_indices: Vec<[u32; 3]> = Vec::new();

    let scenes: Vec<_> =
        if let Some(scene) = gltf.default_scene() { vec![scene] } else { gltf.scenes().collect() };

    for scene in scenes {
        for node in scene.nodes() {
            collect_node(node, IDENTITY, &buffers, &mut all_vertices, &mut all_indices);
        }
    }

    if all_vertices.is_empty() {
        eprintln!("No mesh data found in {:?}", args.input);
        std::process::exit(1);
    }

    println!(
        "Loaded {} vertices, {} triangles — running VHACD...",
        all_vertices.len(),
        all_indices.len()
    );

    let params = VHACDParameters {
        max_convex_hulls: args.max_hulls,
        resolution: args.resolution,
        concavity: args.concavity as f32,
        ..Default::default()
    };

    let decomp = VHACD::decompose(&params, &all_vertices, &all_indices, false);
    let hulls = decomp.compute_convex_hulls(args.downsampling);

    println!("Generated {} convex hulls", hulls.len());

    let mut obj = String::new();
    let mut vertex_offset = 1u32; // OBJ indices are 1-based

    for (i, (verts, tris)) in hulls.iter().enumerate() {
        writeln!(obj, "o hull_{i}")?;
        for v in verts {
            writeln!(obj, "v {} {} {}", v.x, v.y, v.z)?;
        }
        for t in tris {
            let (a, b, c) = (t[0] + vertex_offset, t[1] + vertex_offset, t[2] + vertex_offset);
            writeln!(obj, "f {a} {b} {c}")?;
        }
        vertex_offset += verts.len() as u32;
    }

    std::fs::write(&output, obj)?;
    println!("Written to {:?}", output);

    Ok(())
}
