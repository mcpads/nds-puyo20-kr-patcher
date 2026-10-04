use crate::format::unpack_halfword;
use anyhow::{Result, ensure};

/// COMP + LZ11 for the target's halfword-output reader at ARM9 0x02008098.
/// Distance 1 can read an uncommitted output halfword; use distances 2..4096.
pub fn pack(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        !data.is_empty() && data.len() <= 0xffffff && data.len() % 2 == 0,
        "unsupported LZ11 input size"
    );
    let mut out = b"COMP".to_vec();
    out.extend(((data.len() as u32) << 8 | 0x11).to_le_bytes());
    let mut p = 0;
    while p < data.len() {
        let flag = out.len();
        out.push(0);
        for bit in (0..8).rev() {
            if p == data.len() {
                break;
            }
            let limit = (data.len() - p).min(0x10110);
            let (mut best, mut distance) = (0, 0);
            for d in 2..=p.min(4096) {
                if data[p - d] != data[p] {
                    continue;
                }
                let mut n = 1;
                while n < limit && data[p + n] == data[p + n - d] {
                    n += 1;
                }
                if n > best {
                    best = n;
                    distance = d;
                }
                if best == limit {
                    break;
                }
            }
            if best < 3 {
                out.push(data[p]);
                p += 1;
                continue;
            }
            out[flag] |= 1 << bit;
            let d = distance - 1;
            if best <= 16 {
                out.extend([(((best - 1) << 4) | (d >> 8)) as u8, d as u8]);
            } else if best <= 272 {
                let n = best - 17;
                out.extend([(n >> 4) as u8, ((n << 4) | (d >> 8)) as u8, d as u8]);
            } else {
                let n = best - 273;
                out.extend([
                    (0x10 | (n >> 12)) as u8,
                    (n >> 4) as u8,
                    ((n << 4) | (d >> 8)) as u8,
                    d as u8,
                ]);
            }
            p += best;
        }
    }
    ensure!(
        unpack_halfword(&out)? == data,
        "COMP halfword round trip failed"
    );
    Ok(out)
}
/// Minimum stored size for small sprites, including each eight-token flag byte.
/// Restrict input size to keep the exhaustive match/parse search bounded.
/// Existing large-asset builds keep their original greedy encoding.
pub fn pack_compact(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        !data.is_empty() && data.len() <= 4096 && data.len() % 2 == 0,
        "compact COMP requires 2..4096 even bytes"
    );
    compact(data, 4096)
}

/// Bounded optimal token parse for layout records (up to 64 KiB), with at most 1024-byte matches.
/// The cap bounds search time while allowing better packing than greedy encoding.
pub fn pack_layout(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        !data.is_empty() && data.len() <= 65536 && data.len() % 2 == 0,
        "layout COMP requires 2..65536 even bytes"
    );
    compact(data, 1024)
}

fn compact(data: &[u8], match_limit: usize) -> Result<Vec<u8>> {
    let n = data.len();
    let mut matches = vec![(0usize, 0usize); n];
    for p in 0..n {
        for d in 2..=p.min(4096) {
            let mut len = 0;
            while len < match_limit && p + len < n && data[p + len] == data[p + len - d] {
                len += 1;
            }
            if len > matches[p].0 {
                matches[p] = (len, d);
            }
            if len == n - p {
                break;
            }
        }
    }
    let mut cost = vec![[0usize; 8]; n + 1];
    let mut choices = vec![[1usize; 8]; n];
    for p in (0..n).rev() {
        for slot in 0..8 {
            let next = (slot + 1) % 8;
            let flag = usize::from(slot == 0);
            cost[p][slot] = flag + 1 + cost[p + 1][next];
            for len in 3..=matches[p].0 {
                let bytes = if len <= 16 {
                    2
                } else if len <= 272 {
                    3
                } else {
                    4
                };
                let candidate = flag + bytes + cost[p + len][next];
                if candidate < cost[p][slot] {
                    cost[p][slot] = candidate;
                    choices[p][slot] = len;
                }
            }
        }
    }
    let mut out = b"COMP".to_vec();
    out.extend(((n as u32) << 8 | 0x11).to_le_bytes());
    let mut p = 0;
    while p < n {
        let flag = out.len();
        out.push(0);
        for bit in (0..8).rev() {
            if p == n {
                break;
            }
            let len = choices[p][7 - bit];
            if len == 1 {
                out.push(data[p]);
            } else {
                out[flag] |= 1 << bit;
                let d = matches[p].1 - 1;
                if len <= 16 {
                    out.extend([(((len - 1) << 4) | (d >> 8)) as u8, d as u8]);
                } else if len <= 272 {
                    let x = len - 17;
                    out.extend([(x >> 4) as u8, ((x << 4) | (d >> 8)) as u8, d as u8]);
                } else {
                    let x = len - 273;
                    out.extend([
                        (0x10 | (x >> 12)) as u8,
                        (x >> 4) as u8,
                        ((x << 4) | (d >> 8)) as u8,
                        d as u8,
                    ]);
                }
            }
            p += len;
        }
    }
    ensure!(
        out.len() == 8 + cost[0][0] && unpack_halfword(&out)? == data,
        "compact COMP cost or halfword round trip failed"
    );
    Ok(out)
}

#[cfg(test)]
mod tests;
