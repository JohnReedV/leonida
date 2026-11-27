//! Authored DPP masks through public emission and the emitted integer expressions.
use super::*;
use crate::emit::bitcode::execute;
use crate::native::{ComputeIo, ResourceType, RootParam, RootType, UsedResource};

const OLD: u32 = 0x1234_5678;

#[test]
fn split_wave_counter_has_one_writer_before_the_group_barrier() {
    with_program(true, 32, 15, 15, 64, |lower, _| {
        execute::check_wave_counter(&lower.s.m, lower.wave_base.unwrap(), meeting_mode());
    });
}

fn with_program(
    wave64: bool,
    host_width: u32,
    rows: u8,
    banks: u8,
    threads: u32,
    test: impl FnOnce(Lower<'_>, V),
) {
    let words = [
        0xBE85_0380, // s_mov_b32 s5, 0
        0x3D << 26 | 0x02 << 18 | 8 << 6 | 2,
        125 << 25, // s_load_dwordx4 s[8:11], s[4:5], 0
        0x7E02_02FF,
        OLD,         // v_mov_b32 v1, literal
        0x7E02_02FA, // v_mov_b32 v1, v0 row_mirror
        (0x140 << 8) | (1 << 19) | (u32::from(banks) << 24) | (u32::from(rows) << 28),
        0x3404_0082, // v_lshlrev_b32 v2, 2, v0
        0x38 << 26 | 0x1C << 18 | 1 << 12,
        0x80 << 24 | 2 << 16 | 1 << 8 | 2, // buffer_store_dword v1, v2, s[8:11], 0 offen
        0xBF81_0000,
    ];
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64,
        vgprs: 3,
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
            threads: [threads, 1, 1],
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let p = crate::ir::build(&native.code, wave64, native.vgprs).unwrap();
    let st = crate::structure::structure(&p).unwrap();
    let io = crate::io::raise(&p, &native).unwrap();
    let res = crate::resource::track(&p, &native.layout, native.stage, Some(&st)).unwrap();
    struct Restore(Option<u32>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_HOST_WAVE.set(self.0);
        }
    }
    let _restore = Restore(TEST_HOST_WAVE.replace(Some(host_width)));
    let bytes = crate::dxbc::build(&[]);
    let container = crate::Container::parse(&bytes).unwrap();
    let shader = crate::emit::emit(&container, &native, &p, &st, &io, &res).unwrap();
    crate::Container::parse(&shader.dxil).unwrap();
    #[cfg(windows)]
    if let Ok(mut validator) = crate::emit::validator::Validator::load(
        crate::emit::validator::Validator::default_path().as_deref(),
    ) {
        validator.validate(&mut shader.dxil.clone()).unwrap();
    }
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
    let dpp = p
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .find(|i| matches!(&i.fields, Fields::Valu(v) if v.dpp.is_some()))
        .unwrap();
    let Val::I32(value) = lower.vals[&dpp.write(Reg::V(1)).unwrap()] else {
        panic!("DPP result must be integer");
    };
    test(lower, value);
}

#[test]
fn dpp_rows_follow_original_wave_while_sources_stay_in_the_host_wave() {
    for (wave64, host_width) in [(true, 32), (true, 64), (false, 32)] {
        for rows in [0, 3, 12, 5, 10, 15] {
            for (banks, threads) in [15, 5].into_iter().flat_map(|b| [64, 128].map(|t| (b, t))) {
                with_program(wave64, host_width, rows, banks, threads, |lower, value| {
                    assert_eq!(lower.s.wave_size, Some(host_width));
                    assert_eq!(lower.halves.is_some(), wave64 && host_width == 32);
                    // DPP alone does not need cross-half exchange; test that exact path.
                    assert!(lower.exchange.is_none());
                    for thread in 0..threads {
                        let lane = thread % host_width;
                        let base = thread - lane;
                        let console_lane = thread % if wave64 { 64 } else { 32 };
                        let enabled = (rows >> (console_lane / 16)) & 1 != 0
                            && (banks >> ((console_lane / 4) % 4)) & 1 != 0;
                        let expected = if enabled { thread ^ 15 } else { OLD };
                        let actual = execute::lane_value(&lower.s.m, value, lane, base, host_width);
                        assert_eq!(
                            actual, expected,
                            "wave64={wave64}, host={host_width}, rows={rows:#x}, banks={banks:#x}, thread={thread}"
                        );
                    }
                });
            }
        }
    }
}
