//! Authored wave reduction through public emission; no title input or GPU is used.
use super::*;
use crate::emit::bitcode::execute;
use crate::native::{ComputeIo, ResourceType, RootParam, RootType, UsedResource};

#[test]
fn authored_wave64_mailboxes_have_one_writer_per_half_and_both_barriers() {
    let mut words = vec![
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202,
        0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0
        0x7E0A_0281, // v_mov_b32 v5, 1
        0x2C0C_0081, // v_lshrrev_b32 v6, 1, v0
        0x340E_0B06, // v_lshlrev_b32 v7, v6, v5
    ];
    words.extend_from_slice(&crate::emit::wave::WAVE_OR_V7_TO_S7);
    words.extend_from_slice(&[
        0x7E02_0207, // v_mov_b32 v1, s7
        0x3406_0082, // v_lshlrev_b32 v3, 2, v0
        0xE070_1000,
        0x8002_0103, // buffer_store_dword v1, v3, s[8:11], 0 offen
        0xBF81_0000,
    ]);
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: true,
        vgprs: 16,
        layout: vec![
            RootParam {
                root: RootType::Constants,
                table_offset_dwords: 0,
                sgprs: 4,
                base_register: 0,
                user_data: 0,
                slot: 0,
            },
            RootParam {
                root: RootType::Table,
                table_offset_dwords: 0,
                sgprs: 1,
                base_register: 0,
                user_data: 4,
                slot: 1,
            },
        ],
        used: vec![UsedResource {
            kind: ResourceType::Uav,
            root: RootType::Table,
            table_offset_dwords: 0,
            lower_bound: 0,
            stride: 0,
            dwords: 4,
            user_data: 4,
            slot: 1,
        }],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 5 << 1,
            threads: [64, 1, 1],
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let p = crate::ir::build(&native.code, true, native.vgprs).unwrap();
    let st = crate::structure::structure(&p).unwrap();
    let io = crate::io::raise(&p, &native).unwrap();
    let res = crate::resource::track(&p, &native.layout, native.stage, Some(&st)).unwrap();
    struct Restore(Option<u32>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_HOST_WAVE.set(self.0);
        }
    }
    let _restore = Restore(TEST_HOST_WAVE.replace(Some(32)));
    let bytes = crate::dxbc::build(&[]);
    let container = crate::Container::parse(&bytes).unwrap();
    let shader = crate::emit::emit(&container, &native, &p, &st, &io, &res).unwrap();
    assert!(shader.independent_waves.is_none());
    let emitted = crate::Container::parse(&shader.dxil).unwrap();
    let psv = emitted.part(crate::dxbc::FourCC::PSV0).unwrap().data;
    let word = |at| u32::from_le_bytes(psv[at..at + 4].try_into().unwrap());
    assert_eq!([word(20), word(24)], [32, 32]);
    assert_eq!([word(40), word(44), word(48)], [64, 1, 1]);
    let parts = Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: vec![],
        outputs: vec![],
        prims: vec![],
        mesh_stride: 0,
        independent_waves: None,
    };
    let bad = HashMap::new();
    let mut lower = Lower::new(&parts, &bad);
    assert!(lower.body().is_ok());
    assert!(lower.exchange.is_some() && !lower.local);
    let store = p
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .find(|i| i.name() == "buffer_store_dword")
        .unwrap();
    let Val::I32(output) = lower.vals[&store.read(Reg::V(1)).unwrap()] else {
        panic!("the authored UAV output must be integer")
    };
    execute::check_wave_mailboxes(&lower.s.m, output, meeting_mode());
    lower.s.m.optimise(crate::emit::bitcode::opt::ALL);
    lower.s.m.single_latches();
    execute::check_wave_mailboxes(&lower.s.m, output, meeting_mode());
}
