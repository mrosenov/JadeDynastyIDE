//! Small TGA decoder for standalone client images. Angelica resources commonly
//! use uncompressed or RLE true-colour TGA files with a top- or bottom-left origin.

fn u16_at(data: &[u8], at: usize) -> Result<usize, String> {
    data.get(at..at + 2)
        .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()) as usize)
        .ok_or_else(|| "TGA header is truncated".into())
}

fn pixel(data: &[u8], at: &mut usize, depth: u8, alpha_bits: u8) -> Result<[u8; 4], String> {
    let bytes = usize::from(depth.div_ceil(8));
    let raw = data.get(*at..*at + bytes).ok_or("TGA pixel data is truncated")?;
    *at += bytes;
    Ok(match depth {
        8 => [raw[0], raw[0], raw[0], 255],
        15 | 16 => {
            let value = u16::from_le_bytes([raw[0], raw[1]]);
            let expand = |v: u16| ((v * 255 + 15) / 31) as u8;
            [expand((value >> 10) & 31), expand((value >> 5) & 31), expand(value & 31), if alpha_bits > 0 && value & 0x8000 == 0 { 0 } else { 255 }]
        }
        24 => [raw[2], raw[1], raw[0], 255],
        32 => [raw[2], raw[1], raw[0], raw[3]],
        _ => return Err(format!("unsupported TGA pixel depth {depth}")),
    })
}

/// Decodes a colour or greyscale TGA to top-left-origin RGBA pixels.
pub fn decode(data: &[u8]) -> Result<(Vec<u8>, usize, usize), String> {
    if data.len() < 18 {
        return Err("TGA header is truncated".into());
    }
    let image_type = data[2];
    let rle = matches!(image_type, 10 | 11);
    if !matches!(image_type, 2 | 3 | 10 | 11) {
        return Err(format!("unsupported TGA image type {image_type}"));
    }
    if data[1] != 0 {
        return Err("colour-mapped TGA images are not supported".into());
    }
    let (width, height) = (u16_at(data, 12)?, u16_at(data, 14)?);
    let pixels = width.checked_mul(height).ok_or("TGA dimensions are too large")?;
    if width == 0 || height == 0 || pixels > 64 * 1024 * 1024 {
        return Err(format!("invalid TGA dimensions {width}×{height}"));
    }
    let depth = data[16];
    if matches!(image_type, 3 | 11) && depth != 8 {
        return Err(format!("unsupported greyscale TGA depth {depth}"));
    }
    if matches!(image_type, 2 | 10) && !matches!(depth, 15 | 16 | 24 | 32) {
        return Err(format!("unsupported TGA pixel depth {depth}"));
    }
    let mut at = 18usize.checked_add(data[0] as usize).ok_or("bad TGA image offset")?;
    if at > data.len() {
        return Err("TGA image ID is truncated".into());
    }
    let mut source = Vec::with_capacity(pixels);
    while source.len() < pixels {
        if !rle {
            source.push(pixel(data, &mut at, depth, data[17] & 0x0f)?);
            continue;
        }
        let packet = *data.get(at).ok_or("TGA RLE data is truncated")?;
        at += 1;
        let count = usize::from(packet & 0x7f) + 1;
        if source.len() + count > pixels {
            return Err("TGA RLE packet runs past the image".into());
        }
        if packet & 0x80 != 0 {
            let value = pixel(data, &mut at, depth, data[17] & 0x0f)?;
            source.extend(std::iter::repeat_n(value, count));
        } else {
            for _ in 0..count {
                source.push(pixel(data, &mut at, depth, data[17] & 0x0f)?);
            }
        }
    }

    let top = data[17] & 0x20 != 0;
    let right = data[17] & 0x10 != 0;
    let mut rgba = vec![0u8; pixels * 4];
    for (i, value) in source.into_iter().enumerate() {
        let sx = i % width;
        let sy = i / width;
        let x = if right { width - 1 - sx } else { sx };
        let y = if top { sy } else { height - 1 - sy };
        rgba[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&value);
    }
    Ok((rgba, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(kind: u8, width: u16, height: u16, depth: u8, descriptor: u8) -> Vec<u8> {
        let mut data = vec![0u8; 18];
        data[2] = kind;
        data[12..14].copy_from_slice(&width.to_le_bytes());
        data[14..16].copy_from_slice(&height.to_le_bytes());
        data[16] = depth;
        data[17] = descriptor;
        data
    }

    #[test]
    fn decodes_and_orients_true_colour_pixels() {
        let mut data = header(2, 1, 2, 24, 0); // bottom-left: blue then red
        data.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        let (rgba, width, height) = decode(&data).unwrap();
        assert_eq!((width, height), (1, 2));
        assert_eq!(rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn decodes_rle_packets() {
        let mut data = header(10, 3, 1, 32, 0x28);
        data.extend_from_slice(&[0x82, 3, 2, 1, 128]);
        let (rgba, _, _) = decode(&data).unwrap();
        assert_eq!(rgba, [1, 2, 3, 128, 1, 2, 3, 128, 1, 2, 3, 128]);
    }
}
