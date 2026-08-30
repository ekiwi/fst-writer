// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>
//
// write FST files with fst-writer and read them again with the wellen library
// (using fst-native as the backend)

use fst_writer::*;
use wellen::{SignalRef, Time};

#[test]
fn write_read_simple() {
    let filename = "tests/simple.fst";
    let version = "test 0.2.3";
    let date = "2034-10-10";

    ///////// write
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: version.to_string(),
        date: date.to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    writer
        .scope("simple", "Simple", FstScopeType::Module)
        .unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let b = writer
        .var(
            "b",
            FstSignalType::bit_vec(16),
            FstVarType::Port,
            FstVarDirection::Input,
            None,
        )
        .unwrap();
    let _ = writer
        .var(
            "a_alias",
            FstSignalType::bit_vec(1),
            FstVarType::Port,
            FstVarDirection::Output,
            Some(a),
        )
        .unwrap();
    writer.up_scope().unwrap();

    let mut writer = writer.finish().unwrap();
    // provide an initial value for a
    writer.signal_change(a, b"0").unwrap();
    writer.time_change(1).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.signal_change(b, b"1010101010101010").unwrap();
    writer.time_change(5).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.signal_change(b, b"101010XX10101010").unwrap();

    // flush the buffer, creating a new value change section
    writer.flush().unwrap();

    writer.time_change(7).unwrap();
    writer.signal_change(a, b"X").unwrap();
    writer.signal_change(b, b"0").unwrap();

    writer.time_change(8).unwrap();
    writer.signal_change(a, b"Z").unwrap();

    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();

    // timetable
    assert_eq!(wave.time_table(), [0, 1, 5, 7, 8]);

    // hierarchy
    assert_eq!(wave.hierarchy().date(), date);
    assert_eq!(wave.hierarchy().version(), version);
    {
        let h = wave.hierarchy();
        let top = h.first_scope().unwrap();
        assert_eq!(top.full_name(h), "simple");
        let vars = top.vars(h).map(|r| &h[r]).collect::<Vec<_>>();
        let var_names = vars.iter().map(|v| v.full_name(h)).collect::<Vec<_>>();
        assert_eq!(var_names, ["simple.a", "simple.b", "simple.a_alias"]);
        let signal_ids = vars
            .iter()
            .map(|v| v.signal_ref().index())
            .collect::<Vec<_>>();
        assert_eq!(signal_ids, [0, 1, 0]);
    }

    // signal values
    let (a_ref, b_ref) = (
        SignalRef::from_index(0).unwrap(),
        SignalRef::from_index(1).unwrap(),
    );
    wave.load_signals(&[a_ref, b_ref]);
    let signal_a = wave.get_signal(a_ref).unwrap();
    assert_eq!(signal_a.get_first_time_idx(), Some(0));
    assert_eq!(signal_a.time_indices(), [0, 1, 2, 3, 4]);
    assert_eq!(
        signal_values_to_string(signal_a, wave.time_table()),
        "(0: 0), (1: 1), (5: 0), (7: x), (8: z)"
    );
    let signal_b = wave.get_signal(b_ref).unwrap();
    assert_eq!(
        signal_values_to_string(signal_b, wave.time_table()),
        "(0: xxxxxxxxxxxxxxxx), (1: 1010101010101010), (5: 101010xx10101010), (7: 0000000000000000)"
    );
}

use std::fmt::Write;
fn signal_values_to_string(signal: &wellen::Signal, time_table: &[Time]) -> String {
    let mut out = String::new();
    for (time, value) in signal.iter_changes() {
        write!(
            out,
            "({}: {}), ",
            time_table[time as usize],
            value.to_bit_string().unwrap()
        )
        .unwrap();
    }
    out.pop().unwrap();
    out.pop().unwrap();
    out
}

#[test]
fn write_read_generic_string() {
    let filename = "tests/generic_string.fst";
    let version = "test 0.2.3";
    let date = "2034-10-10";

    ///////// write
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: version.to_string(),
        date: date.to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    writer
        .scope("simple", "Simple", FstScopeType::Module)
        .unwrap();
    let s = writer
        .var(
            "s",
            FstSignalType::variable_length(),
            FstVarType::GenericString,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    writer.up_scope().unwrap();

    // generic string signals have no initial (t=0) value, so the first change
    // must come after an explicit time change
    let mut writer = writer.finish().unwrap();
    writer.time_change(1).unwrap();
    writer.signal_change(s, b"hello").unwrap();
    writer.time_change(5).unwrap();
    writer.signal_change(s, b"world!").unwrap();
    writer.time_change(8).unwrap();
    writer.signal_change(s, b"").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0, 1, 5, 8]);

    let s_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[s_ref]);
    let time_table = wave.time_table().to_vec();
    let signal_s = wave.get_signal(s_ref).unwrap();
    let values: Vec<(Time, String)> = signal_s
        .iter_changes()
        .map(|(time_idx, value)| {
            let value = match value {
                wellen::SignalValueRef::String(v) => v.to_string(),
                other => panic!("expected a string value, got {other:?}"),
            };
            (time_table[time_idx as usize], value)
        })
        .collect();
    assert_eq!(
        values,
        vec![
            (1, "hello".to_string()),
            (5, "world!".to_string()),
            (8, "".to_string()),
        ]
    );
}
