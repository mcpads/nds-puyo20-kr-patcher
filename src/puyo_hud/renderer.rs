use super::*;

pub(super) fn verify(rom: &Rom, ram: Option<&[u8]>) -> Result<Value> {
    let (arm9, _) = crate::arm9::decode(slice(
        rom.bytes,
        u32le(rom.bytes, 0x20)?,
        u32le(rom.bytes, 0x2c)?,
    )?)?;
    let base = u32le(rom.bytes, 0x28)?;
    let overlay = rom
        .overlays
        .iter()
        .find(|o| o.cpu == "arm9" && o.id == 0)
        .ok_or_else(|| anyhow::anyhow!("missing HUD overlay"))?;
    let (ov, _) = crate::arm9::decode(rom.data(&rom.files[overlay.file_id]))?;
    let source = |start, len| {
        if start >= overlay.ram {
            slice(&ov, start - overlay.ram, len)
        } else {
            slice(&arm9, start - base, len)
        }
    };
    let mut windows = Vec::new();
    for (start, end, hash) in [
        (
            0x02136680,
            0x02136748,
            "9ec07a2c306b52109e7f89f2b43098ff150fa69452fb60d42156ab44136b1030",
        ),
        (
            0x02146a00,
            0x02146b9c,
            "d59dd513b274688f5d1d494697ad49e29f4ce877727ff712b5bce5ce01fd63d8",
        ),
        (
            0x021303e0,
            0x02130580,
            "7f8ee164d7bff19c1fae18b96a5dc999e0e6a79327f3c8c10123ddb4d5d686db",
        ),
        (
            0x0213075c,
            0x021308f4,
            "c15635928c022c899e5dc2ab30c85431b21fc41364bd7fafa6cf11628aa23725",
        ),
        (
            0x021471c4,
            0x021472b8,
            "76e076fa5e2927c78bd9b49f2a47a31f678972aec8afaeb1b705cc645a3714f4",
        ),
        (
            0x02147484,
            0x021474cc,
            "83e16451751a5cbb8d8118a8809c6d8b431c5499587ebac146c8c9a4d7a831fd",
        ),
        (
            0x021469ac,
            0x021469f8,
            "35eb24acc20f5bc788b6369312001d60104a2c13da9001a3c432dd5bddfaf8bc",
        ),
        (
            0x021472bc,
            0x02147480,
            "0d023608b1b3bbb32181c14ec3c8ad463cccec69663f01523e8ce841d1f3e141",
        ),
        (
            0x021474cc,
            0x02147614,
            "d2f11219f9839b60ea10cf4063f264cea64f0714276114ee2a866a1f65fc2d86",
        ),
        (
            0x0214761c,
            0x021476fc,
            "d17eb724e86c6594fff27a24d31702ca7e4a11dcb0e19d509aa7ea40a35a8139",
        ),
        (
            0x02147704,
            0x021478f8,
            "0db091ca25cc103d8b92f72351c2c2972daad842354d7ff1dfd245b29aeb8861",
        ),
        (
            0x02147904,
            0x021479cc,
            "00eff9db49bf780d658831f54cf4d094fa8986721628dd996c21f956d9817975",
        ),
        (
            0x0212600c,
            0x02126074,
            "631af5ca7d6cc00354caf919c844842a44b0067d87b5a73f8203fff5c9c20243",
        ),
        (
            0x02147b74,
            0x02147ca8,
            "efbf41cfe4fc943d5357f7c078f6bac211296b7761344aef5164e8e6a07d7244",
        ),
        (
            0x02032e50,
            0x02032f54,
            "f442dd44a36ff9f0769f6bf61745d0f4764f7c2dbd936e9d16d47a8c15d61664",
        ),
        (
            0x02033114,
            0x02033254,
            "25923d60a8d0b15d86d8522d4df4a4e7de90cd2fc7bb0f8783b27a01f510e716",
        ),
        (
            0x02033568,
            0x02033638,
            "0883c06a0cb478d8204a919715dfb394bb248a200cd49537ca89efe8976b8dd6",
        ),
        (
            0x0202f57c,
            0x0202f5b0,
            "400762258da08197868e89956e102af26dd29322142945bc8ba177680de80d66",
        ),
        (
            0x0202f938,
            0x0202fa4c,
            "36be19c2b13b0d362e84c3702f8562f1b466098ddd00c66856098f12b4dbb330",
        ),
        (
            0x0212ef54,
            0x0212f01c,
            "cc7fb85c435f318bbd9a2f62537f7690d0ce69059b377dec92255ee7ab4ae497",
        ),
        (
            0x0212f1c4,
            0x0212f510,
            "415c4dc4d868caa73e20de85f49a0aad843fe9b0d9d844ccc7943c5dbfd86c2c",
        ),
    ] {
        let bytes = source(start, end - start)?;
        ensure!(
            sha(bytes) == hash,
            "HUD renderer source changed at {start:#x}"
        );
        if let Some(ram) = ram {
            ensure!(
                bytes == slice(ram, start - 0x02000000, end - start)?,
                "HUD renderer RAM differs"
            );
        }
        let mut instructions = Vec::new();
        for (i, b) in bytes.chunks_exact(4).enumerate() {
            let op = arm946e_s::decode_arm_bytes(b)?;
            ensure!(
                arm946e_s::encode_arm_bytes(&op)? == b,
                "renderer ARM round trip failed"
            );
            instructions.push(
                json!({"address":start+4*i,"bytes":hex::encode(b),"instruction":format!("{op:?}")}),
            );
        }
        windows.push(json!({"address":start,"sha256":hash,"instructions":instructions}));
    }
    for (address, value) in [
        (0x02033638, 0x040004a8),
        (0x0203363c, 0x040004ac),
        (0x02126184, 646),
        (0x0214caa4, 2),
        (0x0214caa8, 0x00a00040),
        (0x0214caac, 0x03210320),
        (0x021469f8, 0x0214c944),
        (0x0214c944, 3),
        (0x0214c948, 0x00380080),
        (0x0214c94c, 0x03250324),
        (0x0214c954, 3),
        (0x0214c958, 0x003a0080),
        (0x0214c95c, 0x02e302e2),
        (0x0214be98, 1),
        (0x0214be9c, 0x00580040),
        (0x0214bea0, 0x03390338),
        (0x0214bea8, 1),
        (0x0214beac, 0x00600040),
        (0x0214beb0, 0x02810280),
        (0x0214cab4, 1),
        (0x0214cab8, 0x00200040),
        (0x0214cabc, 0x03230322),
        (0x02146ba4, 0x0214fc68),
        (0x0214fc68, 0x20140000),
        (0x0214fc6c, 0x00300000),
        (0x0214fc70, 0x0b0d0a0c),
        (0x0214fc74, 0x20160014),
        (0x0214fc78, 0x0a300015),
        (0x0214fc7c, 0x01231610),
        (0x02136758, 0x0214e5bc),
        (0x0213675c, 0x0214e5d4),
    ] {
        let bytes = source(address, 4)?;
        ensure!(
            u32le(bytes, 0)? == value,
            "texture register literal changed"
        );
        if let Some(ram) = ram {
            ensure!(
                bytes == slice(ram, address - 0x02000000, 4)?,
                "register literal RAM differs"
            );
        }
    }
    Ok(
        json!({"windows":windows,"source_runtime_equal":ram.is_some(),"texture_format":3,"color_zero_transparent":true,"label_crops":[[0,22,26,18],[28,22,26,18],[55,22,26,18],[81,22,26,18]],"claim":"static loader and draw argument trace; not proof of active draw calls or VRAM residency"}),
    )
}
