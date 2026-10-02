//! Minimal DDS reader: decodes a rectangle of the top mip level to RGBA, so
//! single icons can be cut from large atlases without decoding all of them.
//! Supports DXT1, DXT3, DXT5 and uncompressed 32/24-bit textures.

const HEADER: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Format {
    Dxt1,
    Dxt3,
    Dxt5,
    /// Uncompressed, with bytes per pixel and channel masks (r, g, b, a).
    Rgb { bytes: usize, masks: [u32; 4] },
}

pub struct Dds {
    data: Vec<u8>,
    pub width: usize,
    pub height: usize,
    format: Format,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 0x1f) as u32;
    let g = ((c >> 5) & 0x3f) as u32;
    let b = (c & 0x1f) as u32;
    [((r * 255 + 15) / 31) as u8, ((g * 255 + 31) / 63) as u8, ((b * 255 + 15) / 31) as u8]
}

/// The 16 RGBA pixels of a 4×4 colour block. `opaque_only` is set for
/// DXT3/DXT5, whose colour blocks always use the 4-colour mode.
fn color_block(b: &[u8], opaque_only: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u32, wb: u32| ((a as u32 * wa + b as u32 * wb) / (wa + wb)) as u8;
    let mut palette = [[0u8; 4]; 4];
    palette[0] = [p0[0], p0[1], p0[2], 255];
    palette[1] = [p1[0], p1[1], p1[2], 255];
    if c0 > c1 || opaque_only {
        palette[2] = [mix(p0[0], p1[0], 2, 1), mix(p0[1], p1[1], 2, 1), mix(p0[2], p1[2], 2, 1), 255];
        palette[3] = [mix(p0[0], p1[0], 1, 2), mix(p0[1], p1[1], 1, 2), mix(p0[2], p1[2], 1, 2), 255];
    } else {
        palette[2] = [mix(p0[0], p1[0], 1, 1), mix(p0[1], p1[1], 1, 1), mix(p0[2], p1[2], 1, 1), 255];
        palette[3] = [0, 0, 0, 0];
    }
    let bits = u32_at(b, 4);
    let mut out = [[0u8; 4]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        *px = palette[((bits >> (2 * i)) & 3) as usize];
    }
    out
}

fn dxt5_alpha(b: &[u8]) -> [u8; 16] {
    let (a0, a1) = (b[0] as u32, b[1] as u32);
    let mut table = [0u8; 8];
    table[0] = a0 as u8;
    table[1] = a1 as u8;
    if a0 > a1 {
        for i in 1..7 {
            table[i + 1] = (((7 - i) as u32 * a0 + i as u32 * a1) / 7) as u8;
        }
    } else {
        for i in 1..5 {
            table[i + 1] = (((5 - i) as u32 * a0 + i as u32 * a1) / 5) as u8;
        }
        table[6] = 0;
        table[7] = 255;
    }
    let bits = b[2..8].iter().rev().fold(0u64, |acc, &x| (acc << 8) | x as u64);
    let mut out = [0u8; 16];
    for (i, a) in out.iter_mut().enumerate() {
        *a = table[((bits >> (3 * i)) & 7) as usize];
    }
    out
}

fn channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 255;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    (((pixel & mask) >> shift) * 255 / max) as u8
}

impl Dds {
    pub fn parse(data: Vec<u8>) -> Result<Self, String> {
        if data.len() < HEADER || &data[..4] != b"DDS " {
            return Err("not a DDS file".into());
        }
        let height = u32_at(&data, 12) as usize;
        let width = u32_at(&data, 16) as usize;
        let pf_flags = u32_at(&data, 80);
        let format = if pf_flags & 0x4 != 0 {
            match &data[84..88] {
                b"DXT1" => Format::Dxt1,
                b"DXT2" | b"DXT3" => Format::Dxt3,
                b"DXT4" | b"DXT5" => Format::Dxt5,
                other => return Err(format!("unsupported DDS format {}", String::from_utf8_lossy(other))),
            }
        } else {
            let bits = u32_at(&data, 88) as usize;
            if bits != 32 && bits != 24 {
                return Err(format!("unsupported DDS pixel size {bits}"));
            }
            let alpha = if pf_flags & 0x1 != 0 { u32_at(&data, 104) } else { 0 };
            Format::Rgb { bytes: bits / 8, masks: [u32_at(&data, 92), u32_at(&data, 96), u32_at(&data, 100), alpha] }
        };
        let dds = Self { data, width, height, format };
        if dds.data.len() < HEADER + dds.top_level_size() {
            return Err("DDS file is truncated".into());
        }
        Ok(dds)
    }

    fn top_level_size(&self) -> usize {
        let blocks = self.width.div_ceil(4) * self.height.div_ceil(4);
        match self.format {
            Format::Dxt1 => blocks * 8,
            Format::Dxt3 | Format::Dxt5 => blocks * 16,
            Format::Rgb { bytes, .. } => self.width * self.height * bytes,
        }
    }

    /// RGBA pixels of a rectangle (clipped to the texture; outside is transparent).
    pub fn rect(&self, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
        let mut out = vec![0u8; w * h * 4];
        let mut put = |px: usize, py: usize, rgba: [u8; 4]| {
            if px >= x && px < x + w && py >= y && py < y + h && px < self.width && py < self.height {
                let at = ((py - y) * w + (px - x)) * 4;
                out[at..at + 4].copy_from_slice(&rgba);
            }
        };
        let data = &self.data[HEADER..];
        match self.format {
            Format::Rgb { bytes, masks } => {
                for py in y..(y + h).min(self.height) {
                    for px in x..(x + w).min(self.width) {
                        let at = (py * self.width + px) * bytes;
                        let mut v = [0u8; 4];
                        v[..bytes].copy_from_slice(&data[at..at + bytes]);
                        let p = u32::from_le_bytes(v);
                        put(px, py, [channel(p, masks[0]), channel(p, masks[1]), channel(p, masks[2]), channel(p, masks[3])]);
                    }
                }
            }
            format => {
                let block_bytes = if format == Format::Dxt1 { 8 } else { 16 };
                let per_row = self.width.div_ceil(4);
                for by in y / 4..(y + h).div_ceil(4).min(self.height.div_ceil(4)) {
                    for bx in x / 4..(x + w).div_ceil(4).min(per_row) {
                        let b = &data[(by * per_row + bx) * block_bytes..][..block_bytes];
                        let pixels = match format {
                            Format::Dxt1 => color_block(b, false),
                            Format::Dxt3 => {
                                let mut px = color_block(&b[8..], true);
                                for (i, p) in px.iter_mut().enumerate() {
                                    let nibble = (b[i / 2] >> (4 * (i % 2))) & 0xf;
                                    p[3] = nibble * 17;
                                }
                                px
                            }
                            Format::Dxt5 => {
                                let mut px = color_block(&b[8..], true);
                                for (p, a) in px.iter_mut().zip(dxt5_alpha(b)) {
                                    p[3] = a;
                                }
                                px
                            }
                            Format::Rgb { .. } => unreachable!(),
                        };
                        for (i, rgba) in pixels.into_iter().enumerate() {
                            put(bx * 4 + i % 4, by * 4 + i / 4, rgba);
                        }
                    }
                }
            }
        }
        out
    }
}

/// Encodes RGBA pixels as a PNG.
pub fn png(rgba: &[u8], w: usize, h: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dds(fourcc: &[u8; 4], w: u32, h: u32, body: &[u8]) -> Vec<u8> {
        let mut d = vec![0u8; HEADER];
        d[..4].copy_from_slice(b"DDS ");
        d[12..16].copy_from_slice(&h.to_le_bytes());
        d[16..20].copy_from_slice(&w.to_le_bytes());
        d[80..84].copy_from_slice(&4u32.to_le_bytes());
        d[84..88].copy_from_slice(fourcc);
        d.extend_from_slice(body);
        d
    }

    #[test]
    fn decodes_dxt1_blocks() {
        // One block: c0 = pure red, c1 = pure blue, all pixels index 0 except the last (index 1).
        let block = [0x00, 0xf8, 0x1f, 0x00, 0x00, 0x00, 0x00, 0x40];
        let d = Dds::parse(dds(b"DXT1", 4, 4, &block)).unwrap();
        let px = d.rect(0, 0, 4, 4);
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
        assert_eq!(&px[15 * 4..], &[0, 0, 255, 255]);
        // Clipping outside the texture leaves transparent pixels.
        assert_eq!(&d.rect(2, 2, 4, 4)[3 * 4 * 4..], &[0; 16]);
    }

    #[test]
    fn decodes_dxt3_alpha() {
        let mut block = vec![0xffu8; 8]; // fully opaque explicit alpha…
        block[0] = 0x00; // …except pixels 0 and 1
        block.extend_from_slice(&[0x00, 0xf8, 0x00, 0xf8, 0, 0, 0, 0]);
        let d = Dds::parse(dds(b"DXT3", 4, 4, &block)).unwrap();
        let px = d.rect(0, 0, 4, 4);
        assert_eq!(px[3], 0);
        assert_eq!(px[2 * 4 + 3], 255);
        assert_eq!(&px[8..11], &[255, 0, 0]);
    }
}
