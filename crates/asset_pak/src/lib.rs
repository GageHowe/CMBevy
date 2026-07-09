use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{
        Aead, KeyInit,
        rand_core::{OsRng, RngCore},
    },
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "bevy_reader")]
mod bevy_reader;
#[cfg(feature = "bevy_reader")]
pub use bevy_reader::*;

const MAGIC: &[u8; 8] = b"CMPAK001";
const HEADER_LEN: usize = 48;
pub const DEFAULT_PAK_FILE: &str = "encrypted/assets.pak";

#[derive(Clone, Deserialize, Serialize)]
pub struct Entry {
    pub offset: u64,
    pub len: u64,
    pub raw_len: u64,
    pub compressed: bool,
    pub nonce: [u8; 24],
}

#[derive(Deserialize, Serialize)]
struct Manifest {
    entries: BTreeMap<String, Entry>,
}

pub struct Pak {
    path: PathBuf,
    entries: BTreeMap<String, Entry>,
    key: [u8; 32],
}

pub fn key_from_hex(hex: &str) -> Result<[u8; 32], String> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err("pak key must be 64 hex chars".into());
    }
    let mut key = [0; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "pak key must be hex".to_string())?;
    }
    Ok(key)
}

pub fn write(
    output: impl AsRef<Path>,
    files: impl IntoIterator<Item = (String, PathBuf, bool)>,
    key: [u8; 32],
) -> io::Result<()> {
    if let Some(parent) = output.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(output)?;
    out.write_all(&[0; HEADER_LEN])?;
    let cipher = cipher(&key);
    let mut entries = BTreeMap::new();
    for (path, src, compressed) in files {
        let raw = std::fs::read(src)?;
        let data = if compressed {
            zstd::stream::encode_all(raw.as_slice(), 0)?
        } else {
            raw.clone()
        };
        let nonce = nonce();
        let encrypted = cipher
            .encrypt(XNonce::from_slice(&nonce), data.as_slice())
            .map_err(crypto_err)?;
        let offset = out.stream_position()?;
        out.write_all(&encrypted)?;
        entries.insert(
            path,
            Entry {
                offset,
                len: encrypted.len() as u64,
                raw_len: raw.len() as u64,
                compressed,
                nonce,
            },
        );
    }
    let manifest_nonce = nonce();
    let manifest = postcard::to_allocvec(&Manifest { entries }).map_err(data_err)?;
    let manifest = cipher
        .encrypt(XNonce::from_slice(&manifest_nonce), manifest.as_slice())
        .map_err(crypto_err)?;
    let manifest_offset = out.stream_position()?;
    out.write_all(&manifest)?;
    out.seek(SeekFrom::Start(0))?;
    out.write_all(MAGIC)?;
    out.write_all(&manifest_offset.to_le_bytes())?;
    out.write_all(&(manifest.len() as u64).to_le_bytes())?;
    out.write_all(&manifest_nonce)?;
    Ok(())
}

impl Pak {
    pub fn open(path: impl AsRef<Path>, key: [u8; 32]) -> io::Result<Self> {
        let path = path.as_ref().to_owned();
        let mut file = File::open(&path)?;
        let mut header = [0; HEADER_LEN];
        file.read_exact(&mut header)?;
        if &header[..8] != MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bad pak magic"));
        }
        let manifest_offset = u64_at(&header[8..16]);
        let manifest_len = u64_at(&header[16..24]) as usize;
        let mut manifest_nonce = [0; 24];
        manifest_nonce.copy_from_slice(&header[24..48]);
        file.seek(SeekFrom::Start(manifest_offset))?;
        let mut manifest = vec![0; manifest_len];
        file.read_exact(&mut manifest)?;
        let manifest = cipher(&key)
            .decrypt(XNonce::from_slice(&manifest_nonce), manifest.as_slice())
            .map_err(crypto_err)?;
        let manifest: Manifest = postcard::from_bytes(&manifest).map_err(data_err)?;
        Ok(Self {
            path,
            entries: manifest.entries,
            key,
        })
    }

    pub fn entries(&self) -> &BTreeMap<String, Entry> {
        &self.entries
    }

    pub fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let entry = self
            .entries
            .get(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.to_owned()))?;
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(entry.offset))?;
        let mut encrypted = vec![0; entry.len as usize];
        file.read_exact(&mut encrypted)?;
        let data = cipher(&self.key)
            .decrypt(XNonce::from_slice(&entry.nonce), encrypted.as_slice())
            .map_err(crypto_err)?;
        if entry.compressed {
            zstd::stream::decode_all(data.as_slice())
        } else {
            Ok(data)
        }
    }
}

fn cipher(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

fn nonce() -> [u8; 24] {
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

fn u64_at(bytes: &[u8]) -> u64 {
    let mut out = [0; 8];
    out.copy_from_slice(bytes);
    u64::from_le_bytes(out)
}

fn crypto_err(_: chacha20poly1305::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "pak decrypt failed")
}

fn data_err(err: postcard::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("asset_pak_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("textures"))?;
        std::fs::write(root.join("textures/a.txt"), b"hello")?;
        std::fs::write(root.join("textures/b.txt"), b"world")?;
        let pak_path = root.join("assets.pak");
        let key = [7; 32];
        write(
            &pak_path,
            [
                ("textures/a.txt".into(), root.join("textures/a.txt"), true),
                ("textures/b.txt".into(), root.join("textures/b.txt"), false),
            ],
            key,
        )?;
        let pak = Pak::open(&pak_path, key)?;
        assert_eq!(pak.read("textures/a.txt")?, b"hello");
        assert_eq!(pak.read("textures/b.txt")?, b"world");
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }
}
