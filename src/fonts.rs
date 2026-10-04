//! Registered pixel fonts. Galmuri11 Regular and Bold share metrics on the
//! native 12px grid, so tools that draw Galmuri11 accept either weight.

use crate::format::sha;

pub(crate) const GALMURI11_REGULAR_SHA256: &str =
    "2c709890595668f7bdb6df408420fda957dde0288e95b31a1cc17a2ab98b4b4f";
pub(crate) const GALMURI11_BOLD_SHA256: &str =
    "f899e7d8d646a1990a4b6260caa6631b3a50b567ea605ab0047d2d3a955deedb";

/// True for the registered Galmuri11 Regular or Bold file.
pub(crate) fn is_galmuri11(bytes: &[u8]) -> bool {
    let digest = sha(bytes);
    digest == GALMURI11_REGULAR_SHA256 || digest == GALMURI11_BOLD_SHA256
}

/// Galmuri14, drawn on its native 15px grid.
pub(crate) const GALMURI14_SHA256: &str =
    "d3818c0f2898a3b2d79ccd04ec1e4de5e8940aa26abee261f73e315a44ce8df9";

/// BM JUA, the rounded bold display font.
pub(crate) const BMJUA_SHA256: &str =
    "e8e6aa8b1b662c7bf0d7f136f29e822e0985176458a6e5d0ba08afc4a5c901a9";
