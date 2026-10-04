use super::{nds::names, slice, u16le, u32le};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub struct Narc<'a> {
    pub members: Vec<&'a [u8]>,
    pub names: BTreeMap<usize, String>,
}
impl<'a> Narc<'a> {
    pub fn parse(b: &'a [u8]) -> Result<Self> {
        ensure!(
            slice(b, 0, 4)? == b"NARC"
                && u16le(b, 4)? == 0xfffe
                && u32le(b, 8)? == b.len()
                && u16le(b, 12)? == 16
                && u16le(b, 14)? == 3,
            "invalid NARC header"
        );
        let mut p = 16;
        let mut chunks = BTreeMap::new();
        for _ in 0..3 {
            let magic = slice(b, p, 4)?.to_vec();
            let size = u32le(b, p + 4)?;
            ensure!(size >= 8, "invalid NARC section size");
            ensure!(
                chunks.insert(magic, slice(b, p + 8, size - 8)?).is_none(),
                "duplicate NARC section"
            );
            p += size;
        }
        ensure!(p == b.len(), "NARC trailing bytes");
        let get = |k: &[u8]| {
            chunks
                .get(k)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("missing NARC section"))
        };
        let fat = get(b"BTAF")?;
        let img = get(b"GMIF")?;
        let n = u16le(fat, 0)?;
        ensure!(fat.len() == 4 + n * 8, "NARC FAT size mismatch");
        let mut members = Vec::new();
        for i in 0..n {
            let s = u32le(fat, 4 + i * 8)?;
            let e = u32le(fat, 8 + i * 8)?;
            ensure!(s <= e, "reversed NARC extent");
            members.push(slice(img, s, e - s)?);
        }
        // Unnamed Nitro archives use a compact 8-byte root with name offset 4.
        let fnt = get(b"BTNF")?;
        let ns = if fnt == [4, 0, 0, 0, 0, 0, 1, 0] {
            BTreeMap::new()
        } else {
            names(fnt)?
        };
        ensure!(ns.keys().all(|i| *i < n), "NARC name outside member table");
        Ok(Self { members, names: ns })
    }
}
