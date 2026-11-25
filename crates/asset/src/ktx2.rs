//! GPU-ready, uncompressed RGBA8 textures from KTX 2.0 files.

use std::{
    fs, io,
    ops::Range,
    path::{Path, PathBuf},
};

const IDENTIFIER: &[u8; 12] = b"\xabKTX 20\xbb\r\n\x1a\n";
const HEADER_SIZE: usize = 80;
const LEVEL_ENTRY_SIZE: usize = 24;

/// Transfer function applied to RGB on sampling. Alpha is always linear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorSpace {
    /// RGB channels are stored as linear values.
    Linear,
    /// RGB channels are stored as sRGB values.
    Srgb,
}

/// A validated 2D RGBA8 image with its authored mip levels.
#[derive(Debug)]
pub struct Texture {
    source: PathBuf,
    size: [u32; 2],
    color_space: ColorSpace,
    bytes: Vec<u8>,
    levels: Vec<Range<usize>>,
}

impl Texture {
    /// Read an uncompressed linear or sRGB RGBA8 KTX2 file.
    /// Invalid or unsupported data is reported with the source path.
    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
        let (size, color_space, levels) = parse(&bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", path.display()),
            )
        })?;
        Ok(Self {
            source: path.to_owned(),
            size,
            color_space,
            bytes,
            levels,
        })
    }

    /// Path used to load the asset, for diagnostics.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Width and height of the largest level in texels.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// The texture's stored RGB transfer function.
    pub fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    /// Tightly packed RGBA8 texels for each level, largest first.
    pub fn levels(&self) -> impl ExactSizeIterator<Item = &[u8]> {
        self.levels.iter().map(|range| &self.bytes[range.clone()])
    }
}

type Parsed = ([u32; 2], ColorSpace, Vec<Range<usize>>);

fn parse(bytes: &[u8]) -> Result<Parsed, String> {
    let header = bytes.get(..HEADER_SIZE).ok_or("truncated KTX2 header")?;
    if &header[..IDENTIFIER.len()] != IDENTIFIER {
        return Err("invalid KTX2 identifier".into());
    }
    let scheme = word(header, 44);
    if scheme != 0 {
        return Err(format!("unsupported KTX2 supercompression scheme {scheme}"));
    }
    let format = word(header, 12);
    let (color_space, transfer) = match format {
        37 => (ColorSpace::Linear, 1), // VK_FORMAT_R8G8B8A8_UNORM
        43 => (ColorSpace::Srgb, 2),   // VK_FORMAT_R8G8B8A8_SRGB
        _ => return Err(format!("unsupported KTX2 VkFormat {format}")),
    };
    let size = [word(header, 20), word(header, 24)];
    if size.contains(&0)
        || word(header, 16) != 1
        || word(header, 28) != 0
        || word(header, 32) != 0
        || word(header, 36) != 1
    {
        return Err("expected a single 2D RGBA8 image".into());
    }
    let count = word(header, 40);
    if count == 0 || count > u32::BITS - size[0].max(size[1]).leading_zeros() {
        return Err("expected a valid explicit KTX2 mip count".into());
    }
    let index_end = HEADER_SIZE + count as usize * LEVEL_ENTRY_SIZE;
    let index = bytes
        .get(HEADER_SIZE..index_end)
        .ok_or("truncated KTX2 level index")?;
    let dfd_offset = word(header, 48);
    let dfd_range = range(bytes.len(), dfd_offset.into(), word(header, 52).into())
        .filter(|r| r.start == index_end)
        .ok_or("invalid KTX2 data format descriptor range")?;
    validate_dfd(&bytes[dfd_range.clone()], transfer)?;
    let mut data_start = dfd_range.end;
    let kv_offset = word(header, 56);
    let kv_length = word(header, 60);
    if kv_length != 0 {
        let kv = range(bytes.len(), kv_offset.into(), kv_length.into())
            .filter(|r| r.start == data_start)
            .ok_or("invalid KTX2 metadata range")?;
        validate_metadata(&bytes[kv.clone()])?;
        data_start = kv.end;
    } else if kv_offset != 0 {
        return Err("invalid empty KTX2 metadata range".into());
    }
    if wide(header, 64) != 0 || wide(header, 72) != 0 {
        return Err("unexpected KTX2 supercompression data".into());
    }
    let mut levels = Vec::with_capacity(count as usize);
    let mut previous_start = bytes.len();
    for (level, entry) in index.chunks_exact(LEVEL_ENTRY_SIZE).enumerate() {
        let width = (size[0] >> level).max(1);
        let height = (size[1] >> level).max(1);
        let expected = (u64::from(width) * u64::from(height)).checked_mul(4);
        let offset = wide(entry, 0);
        let length = wide(entry, 8);
        if Some(length) != expected || length != wide(entry, 16) || !offset.is_multiple_of(4) {
            return Err(format!("invalid layout for KTX2 mip level {level}"));
        }
        let level_range = range(bytes.len(), offset, length)
            .filter(|r| r.start >= data_start && r.end <= previous_start)
            .ok_or_else(|| format!("invalid KTX2 mip level {level} byte range"))?;
        previous_start = level_range.start;
        levels.push(level_range);
    }
    Ok((size, color_space, levels))
}

fn validate_dfd(dfd: &[u8], transfer: u8) -> Result<(), &'static str> {
    const BASIC_DESCRIPTOR_SIZE: usize = 24 + 4 * 16;
    if dfd.len() < 4 + BASIC_DESCRIPTOR_SIZE
        || word(dfd, 0) as usize != dfd.len()
        || word(dfd, 4) != 0
        || word(dfd, 8) != ((BASIC_DESCRIPTOR_SIZE as u32) << 16 | 2)
        || dfd[12] != 1 // RGBSDA
        || !matches!(dfd[13], 0 | 1) // unspecified or BT.709 / sRGB primaries
        || dfd[14] != transfer
        || dfd[15] != 0 // straight alpha
        || dfd[16..28] != [0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0]
    {
        return Err("unsupported or inconsistent KTX2 data format descriptor");
    }
    Ok(())
}

fn validate_metadata(mut bytes: &[u8]) -> Result<(), &'static str> {
    while !bytes.is_empty() {
        if bytes.len() < 4 {
            return Err("truncated KTX2 metadata entry");
        }
        let length = u64::from(word(bytes, 0));
        let entry = range(bytes.len(), 4, length).ok_or("truncated KTX2 metadata entry")?;
        let data = &bytes[entry];
        let separator = data
            .iter()
            .position(|&b| b == 0)
            .filter(|&i| i != 0)
            .ok_or("invalid KTX2 metadata key")?;
        let value = &data[separator + 1..];
        match &data[..separator] {
            b"KTXorientation" if value != b"rd\0" => return Err("unsupported KTX2 orientation"),
            b"KTXswizzle" if value != b"rgba\0" => return Err("unsupported KTX2 channel swizzle"),
            _ => {}
        }
        let next = range(bytes.len(), 4, length.next_multiple_of(4))
            .ok_or("truncated KTX2 metadata padding")?
            .end;
        bytes = &bytes[next..];
    }
    Ok(())
}

fn range(file_len: usize, offset: u64, length: u64) -> Option<Range<usize>> {
    let start = usize::try_from(offset).ok()?;
    let end = usize::try_from(offset.checked_add(length)?).ok()?;
    (end <= file_len).then_some(start..end)
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn wide(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
