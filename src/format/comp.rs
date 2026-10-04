use super::u32le;
use anyhow::{Result, ensure};

pub fn unpack(b: &[u8]) -> Result<Vec<u8>> {
    decode(b, false)
}

pub fn unpack_halfword(b: &[u8]) -> Result<Vec<u8>> {
    decode(b, true)
}

fn decode(b: &[u8], halfword_output: bool) -> Result<Vec<u8>> {
    if !b.starts_with(b"COMP") {
        return Ok(b.to_vec());
    }
    let method = *b.get(4).ok_or_else(|| anyhow::anyhow!("truncated COMP"))?;
    ensure!(matches!(method, 0x10 | 0x11), "unknown COMP method");
    let mut size = u32le(b, 4)? >> 8;
    let mut p = 8;
    if size == 0 {
        ensure!(
            !halfword_output,
            "extended size unsupported by target halfword reader"
        );
        ensure!(method == 0x11, "zero-size LZ10");
        size = u32le(b, 8)?;
        p = 12;
    }
    ensure!(
        size > 0 && size <= 64 * 1024 * 1024,
        "COMP size outside limit"
    );
    ensure!(
        !halfword_output || size % 2 == 0,
        "uncommitted final halfword"
    );
    let mut out = Vec::with_capacity(size);
    fn take(b: &[u8], p: &mut usize) -> Result<usize> {
        let v = *b
            .get(*p)
            .ok_or_else(|| anyhow::anyhow!("truncated COMP stream"))?;
        *p += 1;
        Ok(v as usize)
    }
    while out.len() < size {
        let flags = take(b, &mut p)?;
        for bit in (0..8).rev() {
            if out.len() == size {
                break;
            }
            if flags & (1 << bit) == 0 {
                out.push(take(b, &mut p)? as u8);
                continue;
            }
            let a = take(b, &mut p)?;
            let c = take(b, &mut p)?;
            let kind = a >> 4;
            let (len, dist) = if method == 0x10 {
                (kind + 3, ((a & 15) << 8 | c) + 1)
            } else if kind == 0 {
                (
                    ((a & 15) << 4 | c >> 4) + 0x11,
                    ((c & 15) << 8 | take(b, &mut p)?) + 1,
                )
            } else if kind == 1 {
                let d = take(b, &mut p)?;
                let e = take(b, &mut p)?;
                (
                    ((a & 15) << 12 | c << 4 | d >> 4) + 0x111,
                    ((d & 15) << 8 | e) + 1,
                )
            } else {
                (kind + 1, ((a & 15) << 8 | c) + 1)
            };
            ensure!(
                dist <= out.len() && len <= size - out.len(),
                "invalid COMP backreference/output overrun"
            );
            for _ in 0..len {
                ensure!(
                    !halfword_output || out.len() - dist < out.len() / 2 * 2,
                    "backreference reads uncommitted output halfword"
                );
                out.push(out[out.len() - dist]);
            }
        }
    }
    ensure!(p == b.len(), "unexplained COMP trailing bytes");
    Ok(out)
}
