//! TP4J anti-piracy bypass written into the raw prefix of the stored ARM9.
//!
//! Some loaders stop after the SEGA logo because the title and battle overlays
//! install anti-piracy check pointers. The ARM9 backward decompressor, which also
//! unpacks every overlay, returns with `BX LR` at `0x020009F8`. The bypass turns
//! that return into a branch to a hook placed in secure-area bytes before the ARM9
//! entry point (`0x02000530`, never executed). After each decompression the hook
//! redirects the two check pointers, only while they still hold their original
//! targets, and returns to the decompressor's caller.
//!
//! The 132 bytes are assembled with the typed ARM946E-S assembler.
//!
//! Both spans lie in the uncompressed ARM9 prefix, so the stored and decoded bytes
//! change identically and the compressed body is untouched.
use crate::{build::Write, format::*};
use anyhow::{Result, ensure};
use arm7tdmi::{
    AddressOffset, AddressingMode2, ArmInstruction as BaseArm, BlockAddressing, Condition,
    DataOperation, IndexMode, Operand2, Register, RegisterList, RotatedImmediate, TransferWidth,
};
use arm946e_s::{ArmAssembler, ArmInstruction, decode_arm_bytes, encode_arm_bytes};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Hook location: secure-area bytes ahead of the ARM9 entry point.
const HOOK: u32 = 0x0200_0530;
const HOOK_SIZE: usize = 0x80;
/// Final `BX LR` of the backward decompressor.
const DECOMPRESSOR_RETURN: u32 = 0x0200_09f8;
/// SHA-256 of the 128 registered TP4J source bytes the hook replaces.
const HOOK_SOURCE_SHA256: &str = "b31351dbc93ccee0f57d3f122f1743ec0b22d42f3b08de404374dbc834ab895d";

enum Item {
    Label(&'static str),
    Code(BaseArm),
    /// PC-relative word load from a labelled literal.
    Load(Condition, u8, &'static str),
    Word(u32),
}

fn register(index: u8) -> Result<Register> {
    Ok(Register::new(index)?)
}

fn return_constant(value: u32) -> Result<Vec<Item>> {
    // The literal sits directly after `BX LR`: PC + 8 reaches it with offset 0.
    Ok(vec![
        Item::Code(word_load(Condition::Always, 0, 0)?),
        Item::Code(BaseArm::BranchExchange {
            condition: Condition::Always,
            target: Register::LINK,
        }),
        Item::Word(value),
    ])
}

fn word_load(condition: Condition, destination: u8, offset: u16) -> Result<BaseArm> {
    word_transfer(condition, true, destination, Register::PC, offset)
}

fn word_transfer(
    condition: Condition,
    load: bool,
    value: u8,
    base: Register,
    offset: u16,
) -> Result<BaseArm> {
    Ok(BaseArm::SingleTransfer {
        condition,
        load,
        width: TransferWidth::Word,
        register: register(value)?,
        address: AddressingMode2 {
            base,
            offset: AddressOffset::Immediate(offset),
            add: true,
            index: IndexMode::Offset,
        },
    })
}

/// Redirect `*slot` and `*(slot + 0x3C)` when `*slot` still equals `original`.
fn redirect(original: &'static str, slot: &'static str, step: u8) -> Result<Vec<Item>> {
    let (r0, r1, r2) = (register(0)?, register(1)?, register(2)?);
    Ok(vec![
        Item::Load(Condition::Always, 0, original),
        Item::Load(Condition::Always, 1, slot),
        Item::Code(word_transfer(Condition::Always, true, 2, r1, 0)?),
        Item::Code(BaseArm::DataProcessing {
            condition: Condition::Always,
            operation: DataOperation::Compare,
            set_flags: true,
            destination: Register::new(0)?,
            first: r0,
            second: Operand2::Register {
                register: r2,
                shift: arm7tdmi::ImmediateShift::LogicalLeft(0),
            },
        }),
        Item::Load(Condition::Equal, 0, "replacement"),
        Item::Code(word_transfer(Condition::Equal, false, 0, r1, 0)?),
        Item::Code(BaseArm::DataProcessing {
            condition: Condition::Equal,
            operation: DataOperation::Add,
            set_flags: false,
            destination: r0,
            first: r0,
            second: Operand2::Immediate(RotatedImmediate::new(step, 0)?),
        }),
        Item::Code(word_transfer(Condition::Equal, false, 0, r1, 0x3c)?),
    ])
}

fn hook_items() -> Result<Vec<Item>> {
    let mut items = Vec::new();
    for value in [0xb3cf, 0xb177, 0xa2dd] {
        items.extend(return_constant(value)?);
    }
    items.push(Item::Label("entry"));
    items.push(Item::Code(BaseArm::BlockTransfer {
        condition: Condition::Always,
        load: false,
        addressing: BlockAddressing::DecrementBefore,
        write_back: true,
        user_registers: false,
        base: Register::SP,
        registers: RegisterList::new(0x4007)?,
    }));
    items.extend(redirect("battle_original", "battle_slot", 0x0c)?);
    items.extend(redirect("title_original", "title_slot", 0x18)?);
    items.push(Item::Code(BaseArm::BlockTransfer {
        condition: Condition::Always,
        load: true,
        addressing: BlockAddressing::IncrementAfter,
        write_back: true,
        user_registers: false,
        base: Register::SP,
        registers: RegisterList::new(0x8007)?,
    }));
    for (label, value) in [
        ("battle_original", 0x0216_1e90),
        ("battle_slot", 0x0216_06fc),
        ("title_original", 0x0216_1100),
        ("title_slot", 0x0215_f4d8),
        ("replacement", 0x0200_2630),
    ] {
        items.push(Item::Label(label));
        items.push(Item::Word(value));
    }
    Ok(items)
}

/// Assembled hook bytes and its entry address.
fn hook() -> Result<(Vec<u8>, u32)> {
    let items = hook_items()?;
    // Every non-label item is one word, so literal offsets resolve before assembly.
    let mut labels = BTreeMap::new();
    let mut at = HOOK;
    for item in &items {
        match item {
            Item::Label(name) => {
                labels.insert(*name, at);
            }
            _ => at += 4,
        }
    }
    let mut assembler = ArmAssembler::new();
    let mut loads = Vec::new();
    let mut at = HOOK;
    for item in &items {
        match item {
            Item::Label(name) => {
                assembler.label(*name);
                continue;
            }
            Item::Code(code) => {
                assembler.emit(ArmInstruction::Armv4T(*code));
            }
            Item::Load(condition, destination, literal) => {
                let offset = labels[literal]
                    .checked_sub(at + 8)
                    .and_then(|d| u16::try_from(d).ok())
                    .filter(|&d| d < 0x1000)
                    .ok_or_else(|| anyhow::anyhow!("literal {literal} out of load range"))?;
                assembler.emit(ArmInstruction::Armv4T(word_load(
                    *condition,
                    *destination,
                    offset,
                )?));
                loads.push((at, *literal));
            }
            Item::Word(value) => {
                assembler.data(value.to_le_bytes().to_vec());
            }
        }
        at += 4;
    }
    let program = assembler.assemble(HOOK)?;
    let bytes = program.bytes().to_vec();
    ensure!(bytes.len() == HOOK_SIZE, "hook size changed");
    // Every literal load must read the labelled word of the assembled program.
    for (at, literal) in loads {
        let target = program
            .label_location(literal)
            .ok_or_else(|| anyhow::anyhow!("missing literal {literal}"))?;
        let span = program
            .instruction_spans()
            .iter()
            .find(|s| s.location == at)
            .ok_or_else(|| anyhow::anyhow!("missing load at {at:#x}"))?;
        let ArmInstruction::Armv4T(BaseArm::SingleTransfer {
            address:
                AddressingMode2 {
                    offset: AddressOffset::Immediate(offset),
                    ..
                },
            ..
        }) = span.instruction
        else {
            anyhow::bail!("load at {at:#x} changed form");
        };
        ensure!(
            at + 8 + u32::from(offset) == target,
            "literal {literal} misread"
        );
    }
    let entry = program
        .label_location("entry")
        .ok_or_else(|| anyhow::anyhow!("missing hook entry"))?;
    Ok((bytes, entry))
}

fn branch(from: u32, to: u32) -> Result<Vec<u8>> {
    let typed = ArmInstruction::Armv4T(BaseArm::Branch {
        condition: Condition::Always,
        link: false,
        displacement: i32::try_from(i64::from(to) - i64::from(from) - 8)?,
    });
    let bytes = encode_arm_bytes(&typed)?.to_vec();
    ensure!(decode_arm_bytes(&bytes)? == typed, "hook branch round trip");
    Ok(bytes)
}

/// Stored ARM9 extent `(rom offset, size)` and the raw prefix length.
fn layout(rom: &[u8]) -> Result<(usize, usize, usize, u32)> {
    let offset = u32le(rom, 0x20)?;
    let entry = u32::try_from(u32le(rom, 0x24)?)?;
    let load = u32::try_from(u32le(rom, 0x28)?)?;
    let size = u32le(rom, 0x2c)?;
    ensure!(load == 0x0200_0000, "ARM9 load address mismatch");
    let stored = slice(rom, offset, size)?;
    let (_, compression) = crate::arm9::decode(stored)?;
    let prefix = compression["prefix_size"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing ARM9 prefix"))? as usize;
    ensure!(
        HOOK + HOOK_SIZE as u32 <= entry,
        "hook would overlap executed ARM9 code"
    );
    Ok((offset, size, prefix, entry))
}

pub(crate) fn writes(rom: &[u8]) -> Result<Vec<Write>> {
    writes_from(rom, HOOK_SOURCE_SHA256)
}

/// Source precondition: the hook bytes have the given identity and the
/// decompressor still ends in `BX LR`; both spans are in the raw prefix.
fn writes_from(rom: &[u8], hook_source_sha256: &str) -> Result<Vec<Write>> {
    let (offset, _, prefix, _) = layout(rom)?;
    let at = |address: u32| offset + (address - 0x0200_0000) as usize;
    let return_offset = (DECOMPRESSOR_RETURN - 0x0200_0000) as usize;
    ensure!(
        return_offset + 4 <= prefix,
        "bypass spans outside the raw ARM9 prefix"
    );
    let hook_source = slice(rom, at(HOOK), HOOK_SIZE)?;
    ensure!(
        sha(hook_source) == hook_source_sha256,
        "anti-piracy hook source bytes mismatch"
    );
    let return_source = slice(rom, at(DECOMPRESSOR_RETURN), 4)?;
    ensure!(
        decode_arm_bytes(return_source)?
            == ArmInstruction::Armv4T(BaseArm::BranchExchange {
                condition: Condition::Always,
                target: Register::LINK,
            }),
        "decompressor return is not BX LR"
    );
    let (code, entry) = hook()?;
    Ok(vec![
        Write {
            offset: at(HOOK),
            before: hook_source.to_vec(),
            after: code,
            label: "anti-piracy hook".into(),
        },
        Write {
            offset: at(DECOMPRESSOR_RETURN),
            before: return_source.to_vec(),
            after: branch(DECOMPRESSOR_RETURN, entry)?,
            label: "decompressor return to anti-piracy hook".into(),
        },
    ])
}

/// The final stored ARM9 with the bypass spans restored to the source bytes.
pub(crate) fn without_bypass(source: &[u8], result: &[u8]) -> Result<Vec<u8>> {
    let (offset, size, _, _) = layout(source)?;
    let mut stored = slice(result, offset, size)?.to_vec();
    for (address, len) in [(HOOK, HOOK_SIZE), (DECOMPRESSOR_RETURN, 4)] {
        let at = (address - 0x0200_0000) as usize;
        stored[at..at + len].copy_from_slice(slice(source, offset + at, len)?);
    }
    Ok(stored)
}

/// Outside the two spans the final stored ARM9 equals `expected` (the source or
/// declared replacement ARM9), and the decoded image carries the assembled code
/// at both spans with no other change.
pub(crate) fn verify(source: &[u8], result: &[u8], expected: &[u8]) -> Result<Value> {
    let (offset, size, prefix, _) = layout(result)?;
    ensure!(
        without_bypass(source, result)? == expected,
        "stored ARM9 differs outside the anti-piracy bypass"
    );
    let (decoded, _) = crate::arm9::decode(slice(result, offset, size)?)?;
    let (base, _) = crate::arm9::decode(expected)?;
    ensure!(decoded.len() == base.len(), "decoded ARM9 size changed");
    let (code, entry) = hook()?;
    let hook_at = (HOOK - 0x0200_0000) as usize;
    let return_at = (DECOMPRESSOR_RETURN - 0x0200_0000) as usize;
    let mut expected = base.clone();
    expected[hook_at..hook_at + HOOK_SIZE].copy_from_slice(&code);
    expected[return_at..return_at + 4].copy_from_slice(&branch(DECOMPRESSOR_RETURN, entry)?);
    ensure!(
        decoded == expected,
        "decoded ARM9 differs outside the anti-piracy bypass"
    );
    Ok(
        json!({"hook":HOOK,"hook_size":HOOK_SIZE,"hook_entry":entry,"hook_sha256":sha(&code),"decompressor_return":DECOMPRESSOR_RETURN,"raw_prefix_size":prefix,"decoded_changes":decoded.iter().zip(&base).filter(|(a,b)|a!=b).count()}),
    )
}

#[cfg(test)]
mod tests;
