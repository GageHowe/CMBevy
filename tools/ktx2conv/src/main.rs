use byteorder::{LittleEndian, WriteBytesExt};
use clap::Parser;
use image::{DynamicImage, ImageBuffer, Rgb};
use std::f32::consts::PI;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "hdri-to-ktx2")]
#[command(about = "Convert an HDRI equirectangular image to a KTX2 cubemap")]
struct Args {
    /// Input HDRI file (.hdr or .exr)
    #[arg(short, long)]
    input: PathBuf,

    /// Output KTX2 file
    #[arg(short, long)]
    output: PathBuf,

    /// Face resolution in pixels (default: 512)
    #[arg(short, long, default_value_t = 512)]
    resolution: u32,
}

// KTX2 cube face order: +X, -X, +Y, -Y, +Z, -Z
// Each face direction: returns (right, up, forward) basis vectors
fn face_basis(face: usize) -> ([f32; 3], [f32; 3], [f32; 3]) {
    match face {
        0 => ([0., 0., -1.], [0., -1., 0.], [1., 0., 0.]), // +X
        1 => ([0., 0., 1.], [0., -1., 0.], [-1., 0., 0.]), // -X
        2 => ([1., 0., 0.], [0., 0., 1.], [0., 1., 0.]),   // +Y
        3 => ([1., 0., 0.], [0., 0., -1.], [0., -1., 0.]), // -Y
        4 => ([1., 0., 0.], [0., -1., 0.], [0., 0., 1.]),  // +Z
        5 => ([-1., 0., 0.], [0., -1., 0.], [0., 0., -1.]), // -Z
        _ => unreachable!(),
    }
}

/// Sample equirectangular image at a 3D direction vector using bilinear filtering.
fn sample_equirect(img: &ImageBuffer<Rgb<f32>, Vec<f32>>, dir: [f32; 3]) -> [f32; 4] {
    let (w, h) = img.dimensions();

    // Normalize direction
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    let d = [dir[0] / len, dir[1] / len, dir[2] / len];

    // Spherical coordinates
    let phi = d[1].asin(); // latitude: -PI/2 to PI/2
    let theta = d[0].atan2(d[2]); // longitude: -PI to PI

    // Map to [0, 1]
    let u = (theta / (2.0 * PI) + 0.5).fract();
    let v = (phi / PI + 0.5).clamp(0.0, 1.0);

    // Bilinear sample
    let x = u * (w as f32 - 1.0);
    let y = v * (h as f32 - 1.0);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let p00 = img.get_pixel(x0, y0).0;
    let p10 = img.get_pixel(x1, y0).0;
    let p01 = img.get_pixel(x0, y1).0;
    let p11 = img.get_pixel(x1, y1).0;

    let mut out = [0f32; 4];
    for i in 0..3 {
        let top = p00[i] * (1.0 - fx) + p10[i] * fx;
        let bot = p01[i] * (1.0 - fx) + p11[i] * fx;
        out[i] = top * (1.0 - fy) + bot * fy;
    }
    out[3] = 1.0; // alpha
    out
}

/// Render one cube face into a flat Vec<f32> (RGBA f32 pixels, row-major).
fn render_face(img: &ImageBuffer<Rgb<f32>, Vec<f32>>, face: usize, res: u32) -> Vec<f32> {
    let (right, up, forward) = face_basis(face);
    let mut pixels = Vec::with_capacity((res * res * 4) as usize);

    for y in 0..res {
        for x in 0..res {
            // UV in [-1, 1]
            let u = (x as f32 + 0.5) / res as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / res as f32 * 2.0 - 1.0;

            let dir = [
                forward[0] + right[0] * u + up[0] * v,
                forward[1] + right[1] * u + up[1] * v,
                forward[2] + right[2] * u + up[2] * v,
            ];

            let rgba = sample_equirect(img, dir);
            pixels.extend_from_slice(&rgba);
        }
    }

    pixels
}

/// Write a minimal KTX2 file.
/// Format: VK_FORMAT_R32G32B32A32_SFLOAT (0x0074 = 116)
/// 6 faces, 1 mip level, no supercompression.
fn write_ktx2(path: &PathBuf, faces: &[Vec<f32>], res: u32) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut w = BufWriter::new(file);

    // --- KTX2 Header ---
    // Identifier (12 bytes)
    let identifier: [u8; 12] = [
        0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
    ];
    w.write_all(&identifier)?;

    let vk_format: u32 = 109; // VK_FORMAT_R32G32B32A32_SFLOAT
    let type_size: u32 = 4; // bytes per channel
    let pixel_width: u32 = res;
    let pixel_height: u32 = res;
    let pixel_depth: u32 = 0;
    let layer_count: u32 = 0; // 0 = not an array
    let face_count: u32 = 6;
    let level_count: u32 = 1;
    let supercompression_scheme: u32 = 0; // none

    w.write_u32::<LittleEndian>(vk_format)?;
    w.write_u32::<LittleEndian>(type_size)?;
    w.write_u32::<LittleEndian>(pixel_width)?;
    w.write_u32::<LittleEndian>(pixel_height)?;
    w.write_u32::<LittleEndian>(pixel_depth)?;
    w.write_u32::<LittleEndian>(layer_count)?;
    w.write_u32::<LittleEndian>(face_count)?;
    w.write_u32::<LittleEndian>(level_count)?;
    w.write_u32::<LittleEndian>(supercompression_scheme)?;

    // Index (5 * 2 * u32 = 40 bytes for dfd, kvd, sgd, level offsets)
    // We'll compute offsets:
    // Header = 12 (id) + 9*4 (fields) + 40 (index) = 12 + 36 + 40 = 88 bytes
    // Level index: 1 entry = 3 * u64 = 24 bytes
    // Total header block = 88 + 24 = 112 bytes
    // DFD block starts at 112 (we write a minimal one)

    // Header (80 bytes) + Level Index (24 bytes for 1 mip) = 104 bytes before DFD
    let header_size: u64 = 104;

    // Minimal DFD (Data Format Descriptor)
    // We'll write just enough to be valid: totalSize=44, one sample
    // dfdTotalSize(u32) + descriptor block
    let dfd_total_size: u32 = 44;
    let dfd_offset: u32 = header_size as u32;
    let dfd_byte_length: u32 = dfd_total_size;

    // KVD: none
    let kvd_offset: u32 = 0;
    let kvd_byte_length: u32 = 0;

    // SGD: none
    let sgd_offset: u64 = 0;
    let sgd_byte_length: u64 = 0;

    // Index: dfdByteOffset, dfdByteLength, kvdByteOffset, kvdByteLength, sgdByteOffset, sgdByteLength
    w.write_u32::<LittleEndian>(dfd_offset)?;
    w.write_u32::<LittleEndian>(dfd_byte_length)?;
    w.write_u32::<LittleEndian>(kvd_offset)?;
    w.write_u32::<LittleEndian>(kvd_byte_length)?;
    w.write_u64::<LittleEndian>(sgd_offset)?;
    w.write_u64::<LittleEndian>(sgd_byte_length)?;

    // Level index (1 level): byteOffset, byteLength, uncompressedByteLength
    let face_bytes = (res * res * 4 * 4) as u64; // 4 channels * 4 bytes
    let level_byte_length = face_bytes * 6;
    // Level data starts after header (104) + dfd (44) = 148, aligned to 8
    let level_byte_offset: u64 = 152; // 148 rounded up to 8-byte alignment

    w.write_u64::<LittleEndian>(level_byte_offset)?;
    w.write_u64::<LittleEndian>(level_byte_length)?;
    w.write_u64::<LittleEndian>(level_byte_length)?; // uncompressed = same (no supercompression)

    // --- DFD Block (44 bytes) ---
    // Minimal SFLOAT DFD for RGBA32
    w.write_u32::<LittleEndian>(dfd_total_size)?; // dfdTotalSize
    // Descriptor block:
    w.write_u16::<LittleEndian>(0)?; // vendorId
    w.write_u16::<LittleEndian>(0)?; // descriptorType (BASICFORMAT=0)
    w.write_u16::<LittleEndian>(2)?; // versionNumber
    w.write_u16::<LittleEndian>(40)?; // descriptorBlockSize (40 bytes)
    w.write_u8(1)?; // colorModel: RGBSDA
    w.write_u8(1)?; // colorPrimaries: BT709
    w.write_u8(1)?; // transferFunction: LINEAR
    w.write_u8(0)?; // flags
    w.write_u8(0)?; // texelBlockDimension0
    w.write_u8(0)?; // texelBlockDimension1
    w.write_u8(0)?; // texelBlockDimension2
    w.write_u8(0)?; // texelBlockDimension3
    // bytesPlane[8]
    w.write_u8(16)?; // 16 bytes per texel (4 * f32)
    for _ in 0..7 {
        w.write_u8(0)?;
    }
    // One sample descriptor (16 bytes) for packed RGBA32F
    // We'll describe it as a single 128-bit sample
    w.write_u16::<LittleEndian>(0)?; // bitOffset
    w.write_u8(127)?; // bitLength (128 bits = 127+1)
    w.write_u8(0)?; // channelType (R)
    w.write_u32::<LittleEndian>(0)?; // samplePosition[4]
    // sampleLower/Upper as f32 0.0 / 1.0
    w.write_u32::<LittleEndian>(0x00000000)?; // 0.0f
    w.write_u32::<LittleEndian>(0x3F800000)?; // 1.0f

    // Padding to reach level_byte_offset (160)
    // We're at 112 + 44 = 156, need 4 bytes padding
    w.write_u32::<LittleEndian>(0)?;

    // --- Level 0 image data: 6 faces, each res*res RGBA f32 ---
    for face in faces {
        for &val in face {
            w.write_f32::<LittleEndian>(val)?;
        }
    }

    w.flush()?;
    Ok(())
}

fn main() {
    let args = Args::parse();

    println!("Loading: {}", args.input.display());

    let img = match image::open(&args.input) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Failed to open input: {e}");
            std::process::exit(1);
        }
    };

    // Convert to Rgb32F (HDR linear float)
    let img_f32: ImageBuffer<Rgb<f32>, Vec<f32>> = match img {
        DynamicImage::ImageRgb32F(buf) => buf,
        other => {
            // hdr/exr loads as Rgb32F; fallback converts (loses HDR range for LDR inputs)
            other.into_rgb32f()
        }
    };

    println!(
        "Input size: {}x{}, rendering {} cube faces at {}x{}...",
        img_f32.width(),
        img_f32.height(),
        6,
        args.resolution,
        args.resolution
    );

    let faces: Vec<Vec<f32>> = (0..6)
        .map(|face| {
            let name = ["+X", "-X", "+Y", "-Y", "+Z", "-Z"][face];
            println!("  Rendering face {name}...");
            render_face(&img_f32, face, args.resolution)
        })
        .collect();

    println!("Writing KTX2: {}", args.output.display());
    if let Err(e) = write_ktx2(&args.output, &faces, args.resolution) {
        eprintln!("Failed to write output: {e}");
        std::process::exit(1);
    }

    println!("Done.");
}
