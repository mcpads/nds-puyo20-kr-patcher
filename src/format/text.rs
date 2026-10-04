use super::{slice, u16le, u32le};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct TextPair {
    pub height: usize,
    pub width: usize,
    pub glyph_count: usize,
    pub stride: usize,
    pub glyphs: Vec<(usize, usize)>,
    pub groups: Vec<usize>,
    pub references: Vec<usize>,
    pub payload: usize,
    pub non_slot_values: BTreeMap<String, usize>,
}
pub fn text_pair(f: &[u8], t: &[u8]) -> Result<TextPair> {
    ensure!(
        slice(f, 0, 4)? == b"FNT\0" && f.len() >= 48,
        "invalid FNT header"
    );
    let h = u32le(f, 4)?;
    let w = u32le(f, 8)?;
    let n = u32le(f, 12)?;
    ensure!(
        h > 0 && w > 0 && w % 2 == 0 && h <= 4096 && w <= 4096,
        "invalid glyph dimensions"
    );
    let stride = 4 + h * (w / 2);
    ensure!(n > 0 && 48 + n * stride == f.len(), "FNT size mismatch");
    ensure!(
        t.len() >= 16 && t.len() % 2 == 0 && u32le(t, 0)? == t.len(),
        "invalid MTX size"
    );
    let word = |p: usize| -> Result<usize> {
        ensure!(p % 4 == 0, "unaligned MTX pointer table");
        u32le(t, p)
    };
    let first = word(4)?;
    let second = word(first)?;
    let payload = word(second)?;
    ensure!(
        4 < first && first < second && second < payload && payload <= t.len() && payload % 4 == 0,
        "invalid MTX hierarchy"
    );
    let groups = (first..second)
        .step_by(4)
        .map(word)
        .collect::<Result<Vec<_>>>()?;
    let refs = (second..payload)
        .step_by(4)
        .map(word)
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        groups
            .iter()
            .all(|p| p % 4 == 0 && *p >= second && *p < payload),
        "invalid MTX group"
    );
    ensure!(
        refs.iter()
            .all(|p| p % 2 == 0 && *p >= payload && *p < t.len()),
        "invalid MTX string reference"
    );
    let mut control = BTreeMap::new();
    for p in (payload..t.len()).step_by(2) {
        let v = u16le(t, p)?;
        if v >= n {
            *control.entry(format!("{v:04X}")).or_insert(0) += 1;
        }
    }
    Ok(TextPair {
        height: h,
        width: w,
        glyph_count: n,
        stride,
        glyphs: (0..n)
            .map(|i| Ok((u16le(f, 48 + i * stride)?, u16le(f, 50 + i * stride)?)))
            .collect::<Result<Vec<_>>>()?,
        groups,
        references: refs,
        payload,
        non_slot_values: control,
    })
}
