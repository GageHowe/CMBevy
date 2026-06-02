use std::{fs::File, io::Write, path::PathBuf};

use clap::Parser;
use image::ImageFormat;

#[derive(Parser)]
#[command(name = "hdri-to-ktx2")]
#[command(about = "Convert an HDRI equirectangular image to a KTX2 cubemap")]
struct Args {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short, long, default_value_t = 512)]
    resolution: u32,
}

fn main() {
    let args = Args::parse();
    let format = match args
        .input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("hdr") => ImageFormat::Hdr,
        Some("exr") => ImageFormat::OpenExr,
        _ => {
            eprintln!("Unsupported input extension: {}", args.input.display());
            std::process::exit(1);
        }
    };
    let bytes = match std::fs::read(&args.input) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("Failed to read input: {e}");
            std::process::exit(1);
        }
    };
    let output = match ktx2conv::convert_bytes_to_ktx2(&bytes, format, args.resolution) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("Failed to convert input: {e}");
            std::process::exit(1);
        }
    };
    match File::create(&args.output).and_then(|mut file| file.write_all(&output)) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Failed to write output: {e}");
            std::process::exit(1);
        }
    }
}
