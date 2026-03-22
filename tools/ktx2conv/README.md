# hdri-to-ktx2

Converts equirectangular HDRI images (`.hdr`, `.exr`) to KTX2 cubemaps.

## Output format

- `VK_FORMAT_R32G32B32A32_SFLOAT` — full HDR float, no compression
- 6 faces, 1 mip level
- Face order: +X, -X, +Y, -Y, +Z, -Z (Vulkan/KTX2 standard)

## Build

```bash
cargo build --release
```

## Usage

```bash
# Basic (512px faces)
hdri-to-ktx2 -i input.hdr -o output.ktx2

# Custom face resolution
hdri-to-ktx2 -i input.hdr -o output.ktx2 --resolution 1024

# EXR input
hdri-to-ktx2 -i input.exr -o output.ktx2 -r 2048
```

## Notes

- Output is uncompressed linear HDR. For GPU-compressed variants (BC6H, ASTC HDR),
  pipe the result through `toktx` or `basisu` from the KTX-Software suite:
  ```bash
  toktx --encode bc7 --cubemap out_compressed.ktx2 output.ktx2
  ```
- Bilinear filtering is used when sampling the equirectangular source.
- The DFD (Data Format Descriptor) in the output is minimal but valid.
  Tools like `ktxinfo` (from KTX-Software) can validate the output.
