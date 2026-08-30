// Copyright 2024 Cornell University
// released under BSD 3-Clause License
//
// Converts test-inputs/vhdl3.vcd (which contains VHDL generic string signals) to FST
// and checks that value changes are preserved, including for a string signal that
// changes again at the same time as other (fixed-width) signals.

use fst_writer::*;
use std::collections::HashMap;
use wellen::*;

type SignalRefMap = HashMap<SignalRef, FstSignalId>;

fn to_scope_type(tpe: ScopeType) -> FstScopeType {
    match tpe {
        ScopeType::VhdlArchitecture => FstScopeType::VhdlArchitecture,
        ScopeType::VhdlRecord => FstScopeType::VhdlRecord,
        other => panic!("unsupported scope type in test: {other:?}"),
    }
}

fn to_var_type(tpe: VarType) -> FstVarType {
    match tpe {
        VarType::Logic => FstVarType::Logic,
        VarType::StdLogic => FstVarType::Bit,
        VarType::StdLogicVector => FstVarType::Logic,
        VarType::StdULogic => FstVarType::Bit,
        VarType::StdULogicVector => FstVarType::Logic,
        VarType::String => FstVarType::GenericString,
        other => panic!("unsupported var type in test: {other:?}"),
    }
}

fn to_signal_type(enc: SignalEncoding) -> FstSignalType {
    match enc {
        SignalEncoding::String => FstSignalType::variable_length(),
        SignalEncoding::BitVector(len) => FstSignalType::bit_vec(len),
        SignalEncoding::Real => FstSignalType::real(),
    }
}

fn write_item<W: std::io::Write + std::io::Seek>(
    hier: &Hierarchy,
    out: &mut FstHeaderWriter<W>,
    map: &mut SignalRefMap,
    item: ItemRef,
) {
    match item {
        ItemRef::Scope(scope_ref) => {
            let scope = &hier[scope_ref];
            out.scope(
                scope.name(hier),
                scope.component(hier).unwrap_or(""),
                to_scope_type(scope.scope_type()),
            )
            .unwrap();
            for item in scope.items(hier) {
                write_item(hier, out, map, item);
            }
            out.up_scope().unwrap();
        }
        ItemRef::Var(var_ref) => {
            let var = &hier[var_ref];
            let signal_tpe = to_signal_type(var.signal_encoding(hier));
            let tpe = to_var_type(var.var_type());
            let alias = map.get(&var.signal_ref()).cloned();
            let fst_id = out
                .var(
                    var.name(hier),
                    signal_tpe,
                    tpe,
                    FstVarDirection::Implicit,
                    alias,
                )
                .unwrap();
            if alias.is_none() {
                map.insert(var.signal_ref(), fst_id);
            }
        }
    }
}

#[test]
fn vhdl3_generic_string() {
    let input = "test-inputs/vhdl3.vcd";
    let output = "tests/vhdl3.fst";

    let wave_in = wellen::simple::read(input).expect("failed to read the input VCD");

    let info = {
        let hier = wave_in.hierarchy();
        FstInfo {
            start_time: *wave_in.time_table().first().unwrap(),
            timescale_exponent: -15, // 1fs
            version: hier.version().to_string(),
            date: hier.date().to_string(),
            file_type: FstFileType::Vhdl,
        }
    };
    let mut out = open_fst(output, &info).unwrap();
    let mut signal_ref_map = SignalRefMap::new();
    for item in wave_in.hierarchy().items() {
        write_item(wave_in.hierarchy(), &mut out, &mut signal_ref_map, item);
    }
    let mut out = out.finish().unwrap();

    // load every signal referenced by the hierarchy
    let mut wave = wave_in;
    let signal_refs: Vec<SignalRef> = signal_ref_map.keys().cloned().collect();
    wave.load_signals(&signal_refs);
    let time_table = wave.time_table().to_vec();

    // collect (time_idx, fst_id, bytes) across all signals, then replay them in time order,
    // mirroring how the 2fst example streams changes (one time_change per distinct time,
    // followed by all signal changes for that time)
    let mut changes: Vec<(TimeTableIdx, FstSignalId, Vec<u8>)> = Vec::new();
    for (&signal_ref, &fst_id) in signal_ref_map.iter() {
        let signal = wave.get_signal(signal_ref).unwrap();
        for (time_idx, value) in signal.iter_changes() {
            let bytes = match value {
                SignalValueRef::Event => Vec::new(),
                SignalValueRef::BitVec(bv) => bv.bit_string().into_bytes(),
                SignalValueRef::String(s) => s.as_bytes().to_vec(),
                SignalValueRef::Real(r) => r.to_le_bytes().to_vec(),
            };
            changes.push((time_idx, fst_id, bytes));
        }
    }
    changes.sort_by_key(|(time_idx, _, _)| *time_idx);

    let mut current_time_idx: Option<TimeTableIdx> = None;
    for (time_idx, fst_id, bytes) in changes {
        if current_time_idx != Some(time_idx) {
            out.time_change(time_table[time_idx as usize]).unwrap();
            current_time_idx = Some(time_idx);
        }
        out.signal_change(fst_id, &bytes).unwrap();
    }
    out.finish().unwrap();

    //// read back and check
    let mut check = wellen::simple::read(output).expect("failed to read the generated FST");
    assert_eq!(check.time_table(), [0, 50_000_000, 100_000_000]);

    let h = check.hierarchy();
    let ee = h.lookup_var(&["test"], &"ee").expect("missing var ee");
    let d = h.lookup_var(&["test", "rr"], &"d").expect("missing var d");
    let a = h.lookup_var(&["test", "rr"], &"a").expect("missing var a");
    let refs = [h[ee].signal_ref(), h[d].signal_ref(), h[a].signal_ref()];
    check.load_signals(&refs);
    let time_table = check.time_table().to_vec();

    let string_values = |signal_ref: SignalRef| -> Vec<(Time, String)> {
        let signal = check.get_signal(signal_ref).unwrap();
        signal
            .iter_changes()
            .map(|(idx, v)| {
                let s = match v {
                    SignalValueRef::String(s) => s.to_string(),
                    other => panic!("expected a string value, got {other:?}"),
                };
                (time_table[idx as usize], s)
            })
            .collect()
    };

    // "ee" changes at all three timestamps: foo -> bar -> foo
    assert_eq!(
        string_values(refs[0]),
        vec![
            (0, "foo".to_string()),
            (50_000_000, "bar".to_string()),
            (100_000_000, "foo".to_string()),
        ]
    );
    // "d" only changes at the first and last timestamp: foo -> bar
    assert_eq!(
        string_values(refs[1]),
        vec![(0, "foo".to_string()), (100_000_000, "bar".to_string())]
    );

    // sanity check that regular logic signals are unaffected by the string handling
    let signal_a = check.get_signal(refs[2]).unwrap();
    let a_values: Vec<(Time, String)> = signal_a
        .iter_changes()
        .map(|(idx, v)| {
            let s = match v {
                SignalValueRef::BitVec(bv) => bv.bit_string(),
                other => panic!("expected a bit vector value, got {other:?}"),
            };
            (time_table[idx as usize], s)
        })
        .collect();
    assert_eq!(
        a_values,
        vec![(0, "u".to_string()), (100_000_000, "1".to_string())]
    );
}
