//! Preserve the two independently placed FFFE tokens in the verified dialog.
use super::*;

// (line index, prefix): false anchors immediately before the terminator.
pub(super) type ControlAnchors = Vec<(usize, bool)>;

pub(super) fn span(bytes: &[u8]) -> Result<(Vec<u8>, ControlAnchors)> {
    ensure!(bytes.len() % 2 == 0, "odd dialog span");
    let units = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect::<Vec<_>>();
    let mut prose = Vec::new();
    let mut anchors = Vec::new();
    let mut row = 0;
    for (i, &v) in units.iter().enumerate() {
        if v == 0xfffe {
            let prefix = i > 0 && units[i - 1] == 0xfffd;
            let suffix = units.get(i + 1) == Some(&0xffff);
            ensure!(prefix != suffix, "unverified dialog FFFE position");
            ensure!(!anchors.contains(&(row, prefix)), "duplicate dialog FFFE");
            anchors.push((row, prefix));
        } else {
            prose.extend(v.to_le_bytes());
            row += usize::from(v == 0xfffd);
        }
    }
    Ok((prose, anchors))
}

pub(super) fn lines(lines: &[String], anchors: &[(usize, bool)]) -> Result<Vec<String>> {
    let mut prose = lines.to_vec();
    for &(row, prefix) in anchors {
        let line = prose
            .get_mut(row)
            .ok_or_else(|| anyhow::anyhow!("missing dialog control row"))?;
        *line = if prefix {
            line.strip_prefix("{FFFE}")
        } else {
            line.strip_suffix("{FFFE}")
        }
        .ok_or_else(|| anyhow::anyhow!("dialog control anchor moved"))?
        .to_string();
    }
    ensure!(
        prose.iter().all(|s| !s.contains(['{', '}'])),
        "unsupported dialog token"
    );
    Ok(prose)
}
