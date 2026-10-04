use super::*;
use crate::build::apply;

const ARM9_AT: usize = 0x200;

/// Synthetic ROM: header fields and a compressed ARM9 whose decompressor
/// return is `BX LR`; no game bytes.
fn fixture(minimum_prefix: usize) -> Result<Vec<u8>> {
    let mut code: Vec<u8> = (0..0x1000u32).map(|i| (i * 7 + 3) as u8).collect();
    let return_at = (DECOMPRESSOR_RETURN - 0x0200_0000) as usize;
    code[return_at..return_at + 4].copy_from_slice(&encode_arm_bytes(&ArmInstruction::Armv4T(
        BaseArm::BranchExchange {
            condition: Condition::Always,
            target: Register::LINK,
        },
    ))?);
    code.extend(b"compressible ARM9 body ".repeat(200));
    let stored = crate::arm9::encode(&code, minimum_prefix, None)?;
    let mut rom = vec![0; ARM9_AT + stored.len()];
    put32(&mut rom, 0x20, ARM9_AT)?;
    put32(&mut rom, 0x24, 0x0200_0800)?;
    put32(&mut rom, 0x28, 0x0200_0000)?;
    put32(&mut rom, 0x2c, stored.len())?;
    rom[ARM9_AT..].copy_from_slice(&stored);
    Ok(rom)
}

fn hook_sha(rom: &[u8]) -> Result<String> {
    Ok(sha(slice(rom, ARM9_AT + 0x530, HOOK_SIZE)?))
}

#[test]
fn bypass_changes_only_its_spans_and_verifies() -> Result<()> {
    let source = fixture(0x1000)?;
    let mut writes = writes_from(&source, &hook_sha(&source)?)?;
    let result = apply(&source, &mut writes)?;
    verify(&source, &result, &source[ARM9_AT..])?;
    let changed: Vec<usize> = (0..source.len())
        .filter(|&i| source[i] != result[i])
        .collect();
    assert!(!changed.is_empty());
    assert!(changed.iter().all(|&i| {
        let at = i - ARM9_AT;
        (0x530..0x5b0).contains(&at) || (0x9f8..0x9fc).contains(&at)
    }));
    assert_eq!(without_bypass(&source, &result)?, &source[ARM9_AT..]);
    Ok(())
}

#[test]
fn rejects_source_precondition_mismatch() -> Result<()> {
    let source = fixture(0x1000)?;
    let expected = hook_sha(&source)?;
    let mut hook = source.clone();
    hook[ARM9_AT + 0x540] ^= 1;
    let error = writes_from(&hook, &expected).err().unwrap().to_string();
    assert!(error.contains("hook source bytes mismatch"), "{error}");
    let mut ret = source.clone();
    ret[ARM9_AT + 0x9f8] ^= 1;
    let error = writes_from(&ret, &expected).err().unwrap().to_string();
    assert!(error.contains("not BX LR"), "{error}");
    Ok(())
}

#[test]
fn rejects_spans_in_the_compressed_body() -> Result<()> {
    let source = fixture(0x400)?;
    let error = writes_from(&source, &hook_sha(&source)?)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("raw ARM9 prefix"), "{error}");
    Ok(())
}

#[test]
fn verify_rejects_other_decoded_changes() -> Result<()> {
    let source = fixture(0x1000)?;
    let mut writes = writes_from(&source, &hook_sha(&source)?)?;
    let mut result = apply(&source, &mut writes)?;
    result[ARM9_AT + 0xa00] ^= 1;
    let error = verify(&source, &result, &source[ARM9_AT..])
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("outside the anti-piracy bypass"), "{error}");
    Ok(())
}
