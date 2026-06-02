use std::{
    f32::consts::PI,
    io::{BufWriter, Write},
};

use byteorder::{LittleEndian, WriteBytesExt};
use image::{DynamicImage, ImageBuffer, ImageReader, Rgb};

fn face_basis(face: usize) -> ([f32; 3], [f32; 3], [f32; 3]) {
    match face {
        0 => ([0., 0., -1.], [0., -1., 0.], [1., 0., 0.]),
        1 => ([0., 0., 1.], [0., -1., 0.], [-1., 0., 0.]),
        2 => ([1., 0., 0.], [0., 0., 1.], [0., 1., 0.]),
        3 => ([1., 0., 0.], [0., 0., -1.], [0., -1., 0.]),
        4 => ([1., 0., 0.], [0., -1., 0.], [0., 0., 1.]),
        5 => ([-1., 0., 0.], [0., -1., 0.], [0., 0., -1.]),
        _ => unreachable!(),
    }
}

fn sample_equirect(img: &ImageBuffer<Rgb<f32>, Vec<f32>>, dir: [f32; 3]) -> [f32; 4] {
    let (w, h) = img.dimensions();
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    let d = [dir[0] / len, dir[1] / len, dir[2] / len];
    let phi = d[1].asin();
    let theta = d[0].atan2(d[2]);
    let u = (theta / (2.0 * PI) + 0.5).fract();
    let v = (phi / PI + 0.5).clamp(0.0, 1.0);
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
    let mut out = [0.0; 4];
    for i in 0..3 {
        let top = p00[i] * (1.0 - fx) + p10[i] * fx;
        let bottom = p01[i] * (1.0 - fx) + p11[i] * fx;
        out[i] = top * (1.0 - fy) + bottom * fy;
    }
    out[3] = 1.0;
    out
}

fn render_face(img: &ImageBuffer<Rgb<f32>, Vec<f32>>, face: usize, res: u32) -> Vec<f32> {
    let (right, up, forward) = face_basis(face);
    let mut pixels = Vec::with_capacity((res * res * 4) as usize);
    for y in 0..res {
        for x in 0..res {
            let u = (x as f32 + 0.5) / res as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / res as f32 * 2.0 - 1.0;
            let dir = [
                forward[0] + right[0] * u + up[0] * v,
                forward[1] + right[1] * u + up[1] * v,
                forward[2] + right[2] * u + up[2] * v,
            ];
            pixels.extend_from_slice(&sample_equirect(img, dir));
        }
    }
    pixels
}

fn write_ktx2<W: Write>(writer: W, faces: &[Vec<f32>], res: u32) -> std::io::Result<()> {
    let mut w = BufWriter::new(writer);
    w.write_all(&[
        0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
    ])?;
    for value in [109, 4, res, res, 0, 0, 6, 1, 0] {
        w.write_u32::<LittleEndian>(value)?;
    }
    w.write_u32::<LittleEndian>(104)?;
    w.write_u32::<LittleEndian>(44)?;
    w.write_u32::<LittleEndian>(0)?;
    w.write_u32::<LittleEndian>(0)?;
    w.write_u64::<LittleEndian>(0)?;
    w.write_u64::<LittleEndian>(0)?;
    let face_bytes = (res * res * 4 * 4) as u64;
    let level_byte_length = face_bytes * 6;
    w.write_u64::<LittleEndian>(152)?;
    w.write_u64::<LittleEndian>(level_byte_length)?;
    w.write_u64::<LittleEndian>(level_byte_length)?;
    w.write_u32::<LittleEndian>(44)?;
    w.write_u16::<LittleEndian>(0)?;
    w.write_u16::<LittleEndian>(0)?;
    w.write_u16::<LittleEndian>(2)?;
    w.write_u16::<LittleEndian>(40)?;
    for value in [1, 1, 1, 0, 0, 0, 0, 0, 16] {
        w.write_u8(value)?;
    }
    for _ in 0..7 {
        w.write_u8(0)?;
    }
    w.write_u16::<LittleEndian>(0)?;
    w.write_u8(127)?;
    w.write_u8(0)?;
    w.write_u32::<LittleEndian>(0)?;
    w.write_u32::<LittleEndian>(0)?;
    w.write_u32::<LittleEndian>(0x3F800000)?;
    w.write_u32::<LittleEndian>(0)?;
    for face in faces {
        for &value in face {
            w.write_f32::<LittleEndian>(value)?;
        }
    }
    w.flush()?;
    Ok(())
}

pub fn convert_image_to_ktx2_bytes(img: DynamicImage, resolution: u32) -> std::io::Result<Vec<u8>> {
    let img = match img {
        DynamicImage::ImageRgb32F(buf) => buf,
        other => other.into_rgb32f(),
    };
    let faces: Vec<Vec<f32>> = (0..6).map(|face| render_face(&img, face, resolution)).collect();
    let mut bytes = Vec::new();
    write_ktx2(&mut bytes, &faces, resolution)?;
    Ok(bytes)
}

pub fn convert_bytes_to_ktx2(
    bytes: &[u8],
    format: image::ImageFormat,
    resolution: u32,
) -> Result<Vec<u8>, image::ImageError> {
    let img = ImageReader::with_format(std::io::Cursor::new(bytes), format).decode()?;
    convert_image_to_ktx2_bytes(img, resolution).map_err(image::ImageError::IoError)
}

pub fn convert_hdr_bytes_to_ktx2(
    bytes: &[u8],
    resolution: u32,
) -> Result<Vec<u8>, image::ImageError> {
    convert_bytes_to_ktx2(bytes, image::ImageFormat::Hdr, resolution)
}
