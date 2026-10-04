mod comp;
mod narc;
mod nds;
mod source;
mod text;

pub use comp::{unpack, unpack_halfword};
pub use narc::Narc;
pub use nds::{Entry, Rom, banner};
pub use source::{Profile, load};
pub use text::text_pair;

use anyhow::Result;
use sha2::{Digest, Sha256};

pub fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}
pub fn slice(b: &[u8], p: usize, n: usize) -> Result<&[u8]> {
    b.get(
        p..p.checked_add(n)
            .ok_or_else(|| anyhow::anyhow!("extent overflow"))?,
    )
    .ok_or_else(|| anyhow::anyhow!("extent outside input: {p:#x}+{n:#x}"))
}
pub fn u16le(b: &[u8], p: usize) -> Result<usize> {
    Ok(u16::from_le_bytes(slice(b, p, 2)?.try_into()?) as usize)
}
pub fn u32le(b: &[u8], p: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(slice(b, p, 4)?.try_into()?) as usize)
}
pub fn put32(b: &mut [u8], p: usize, value: usize) -> Result<()> {
    let v = u32::try_from(value)?;
    b.get_mut(p..p + 4)
        .ok_or_else(|| anyhow::anyhow!("write outside buffer"))?
        .copy_from_slice(&v.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests;
