// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>
//
// write FST files with fst-writer and read them again with the wellen library
// (using fst-native as the backend)

use fst_writer::*;
use wellen::{SignalRef, Time};

#[test]
fn write_read_empty() {
    let filename = "tests/empty.fst";
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

    let _var = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let writer = writer.finish().unwrap();

    writer.finish().unwrap();

    drop(wellen::simple::read(filename).unwrap());
}

/// The reference implementation mocks up a time zero step if the time never advances,
/// instead of dropping the values.
#[test]
fn write_read_no_time_change() {
    let filename = "tests/no_time_change.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
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
    // c never receives a value and thus stays at its default
    let _c = writer
        .var(
            "c",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    // values are provided, but the time never advances
    writer.signal_change(a, b"1").unwrap();
    writer.signal_change(b, b"1010101010101010").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0]);

    // a, b and c are the first three signals in the file
    let refs = (0..3)
        .map(|ii| SignalRef::from_index(ii).unwrap())
        .collect::<Vec<_>>();
    wave.load_signals(&refs);
    let values = refs
        .iter()
        .map(|r| signal_values_to_string(wave.get_signal(*r).unwrap(), wave.time_table()))
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        ["(0: 1)", "(0: 1010101010101010)", "(0: x)"],
        "the values written before the first time change need to be preserved"
    );
}

/// Calling `finish` right after a `flush` must not write out a second, redundant section.
///
/// The history has to reach three time steps before the flush, or the flush is held back outright
/// (see `SignalBuffer::request_flush`) and there is nothing to test.
#[test]
fn write_read_flush_then_finish() {
    let filename = "tests/flush_then_finish.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.time_change(1).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(2).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.time_change(3).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.flush().unwrap();
    // no more value changes, and flushing again should be a no-op
    writer.flush().unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0, 1, 2, 3]);
    let a_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[a_ref]);
    assert_eq!(
        signal_values_to_string(wave.get_signal(a_ref).unwrap(), wave.time_table()),
        "(0: 0), (1: 1), (2: 0), (3: 1)"
    );
}

/// If no value is written before the first time change, the first value change section starts at
/// that first time, matching the reference. Starting the section at 0 instead would make readers
/// re-create a time 0 entry from the section start time, which shows up as an extra all-`x` sample
/// that a reference-written file does not have.
#[test]
fn write_read_first_time_change_not_zero() {
    let filename = "tests/first_time_change_not_zero.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    // no values before the first time change, and the first time change is not at 0
    writer.time_change(10).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(20).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    let a_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[a_ref]);
    let values = signal_values_to_string(wave.get_signal(a_ref).unwrap(), wave.time_table());

    // this is what a reference-written file looks like
    assert_eq!(
        wave.time_table(),
        [10, 20],
        "no time 0 entry expected, values are: {values}"
    );
    assert_eq!(values, "(10: 1), (20: 0)");
}

/// A first time change at 0 is recorded in the time table, just like in the reference, which means
/// that the frame is not read back: readers only consult it when the first time table entry is
/// past the start time of the section. Signals without a value change thus have no data at all,
/// which is also what a reference-written file looks like.
#[test]
fn write_read_first_time_change_at_zero() {
    let filename = "tests/first_time_change_at_zero.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    // b never receives a value
    let _b = writer
        .var(
            "b",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    // the first time change is at 0, and no value is written before it
    writer.time_change(0).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(1).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0, 1]);
    let (a_ref, b_ref) = (
        SignalRef::from_index(0).unwrap(),
        SignalRef::from_index(1).unwrap(),
    );
    wave.load_signals(&[a_ref, b_ref]);
    assert_eq!(
        signal_values_to_string(wave.get_signal(a_ref).unwrap(), wave.time_table()),
        "(0: 1), (1: 0)"
    );
    assert_eq!(
        wave.get_signal(b_ref).unwrap().get_first_time_idx(),
        None,
        "a signal without any value change has no data"
    );
}

/// Values written before a first time change at 0 are lost, and that is what the reference does
/// too: before the first time change a value only ever reaches the frame, and readers skip the
/// frame when the first time step sits at the start time of the section. A signal that never
/// received a value has no data either.
#[test]
fn write_read_value_before_time_change_at_zero() {
    let filename = "tests/value_before_time_change_at_zero.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
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
    // c never receives a value
    let _c = writer
        .var(
            "c",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    // initial values, followed by a first time change at 0
    writer.signal_change(a, b"1").unwrap();
    writer.signal_change(b, b"1010101010101010").unwrap();
    writer.time_change(0).unwrap();
    writer.time_change(5).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [0, 5]);
    let refs = (0..3)
        .map(|ii| SignalRef::from_index(ii).unwrap())
        .collect::<Vec<_>>();
    wave.load_signals(&refs);
    // only the change written after the first time step survives; the `1` written before it went
    // into the skipped frame
    assert_eq!(
        signal_values_to_string(wave.get_signal(refs[0]).unwrap(), wave.time_table()),
        "(5: 0)"
    );
    for (r, name) in refs[1..].iter().zip(["b", "c"]) {
        assert_eq!(
            wave.get_signal(*r).unwrap().get_first_time_idx(),
            None,
            "{name} has no value change of its own, so it has no data"
        );
    }
}

/// A section that holds no value change at all is never written out: the reference never
/// finalizes such a section, leaving its header tagged as a skip block, which readers treat like
/// EOF. Here the only value written lands in the frame, which is then skipped, so nothing is left.
///
/// A file with no value change section is barely usable — readers reject it outright, and wellen
/// panics as soon as signals are loaded — but it is what the reference produces, see
/// `fstapi_diff_no_value_changes` in `tests/fstapi_read.rs`.
#[test]
fn write_read_value_then_time_change_at_zero_writes_no_section() {
    let filename = "tests/value_then_time_change_at_zero.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(0).unwrap();
    writer.finish().unwrap();

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    assert!(
        wave.time_table().is_empty(),
        "no value change section was written: {:?}",
        wave.time_table()
    );
    // no `load_signals` here: wellen panics on an empty time table
}

/// Time changes on their own do not make a section either (see above). The time steps are simply
/// absent from the file, while the header still records the end time, as the reference does.
#[test]
fn write_read_only_time_changes_writes_no_section() {
    let filename = "tests/only_time_changes.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let _a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();

    let mut writer = writer.finish().unwrap();
    writer.time_change(0).unwrap();
    writer.time_change(5).unwrap();
    writer.finish().unwrap();

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    assert!(
        wave.time_table().is_empty(),
        "no value change section was written: {:?}",
        wave.time_table()
    );
    // no `load_signals` here: wellen panics on an empty time table
}

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

    // held back by `SignalBuffer::request_flush`, the section has two time steps, needs three
    writer.flush().unwrap();

    writer.time_change(7).unwrap();
    writer.signal_change(a, b"X").unwrap();
    writer.signal_change(b, b"0").unwrap();

    writer.time_change(8).unwrap();
    writer.signal_change(a, b"Z").unwrap();

    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();

    // time table
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

/// Regression test for the situation where a compressed time table have the same size as the raw
/// time table. In those situations the uncompressed version should be written, otherwise the reader
/// would interpret the compresed time table as raw.
///
/// At the time of writing, this happens at step 11. Use a sweep to hopefully make it more stable
/// across internal changes.
#[test]
fn write_read_time_table_sizes() {
    let filename = "tests/time_table_sizes.fst";

    for steps in 1..=40u64 {
        let context = format!("{steps} time steps");
        let info = FstInfo {
            start_time: 0,
            timescale_exponent: 0,
            version: "test 0.2.3".to_string(),
            date: "2034-10-10".to_string(),
            file_type: FstFileType::Verilog,
        };
        let mut writer = open_fst(filename, &info).unwrap();
        let a = writer
            .var(
                "a",
                FstSignalType::bit_vec(1),
                FstVarType::Logic,
                FstVarDirection::Implicit,
                None,
            )
            .unwrap();

        let mut writer = writer.finish().unwrap();

        let times: Vec<u64> = (0..=steps).collect();
        for time in &times {
            writer.time_change(*time).unwrap();
            // alternating, so the history is the same whether or not repeats are suppressed
            writer
                .signal_change(a, if time % 2 == 0 { b"0" } else { b"1" })
                .unwrap();
        }
        writer.finish().unwrap();

        //// read
        // this is the failure the bug produced: the file cannot be opened at all
        let wave = wellen::simple::read(filename)
            .unwrap_or_else(|e| panic!("{context}: the file could not be read back: {e:?}"));

        assert_eq!(
            wave.time_table(),
            times.as_slice(),
            "{context}: wrong time table"
        );
    }
}

/// A real valued signal that was never written reads back as NaN, not as eight `x` bytes.
#[test]
fn write_read_real_initial_value() {
    let filename = "tests/real_initial_value.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    writer
        .var(
            "r",
            FstSignalType::real(),
            FstVarType::Real,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    // a second signal, so that the section holds a value change even though `r` is never written
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    // written before the first time change so the section starts at 0 while the first time chain
    // starts at 10, forcing the reader to read the section `frame`, where `r` is initialized as
    // NaN.
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(10).unwrap();
    // write another value to ensure the section is not empty and dropped.
    writer.signal_change(a, b"0").unwrap();
    writer.finish().unwrap();

    //// read
    let mut wave = wellen::simple::read(filename).unwrap();
    // `r` is declared first, so it is signal index 0
    let r_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[r_ref]);
    let (_, value) = wave
        .get_signal(r_ref)
        .unwrap()
        .iter_changes()
        .next()
        .expect("the frame has to give `r` an initial value");
    match value {
        wellen::SignalValueRef::Real(v) => assert!(
            v.is_nan(),
            "an untouched real reads back as NaN, not as eight `x` bytes; got {v:?}"
        ),
        other => panic!("expected a real, got {other:?}"),
    }
}

/// A character that is not one of the nine `std_logic` states is rejected rather than silently
/// written.
///
/// `?` is the interesting one: the format does have a code for it, but the reference reserves that
/// code for future expansion and no reader can decode it as a value.
#[test]
fn write_invalid_bit_vector_character() {
    let filename = "tests/invalid_character.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();

    // a time change that actually opens a time step, so the value is encoded right away
    writer.time_change(1).unwrap();
    for bad in ['q', '?'] {
        let err = writer
            .signal_change(a, bad.to_string().as_bytes())
            .unwrap_err();
        assert!(
            matches!(err, FstWriteError::InvalidCharacter(c) if c == bad),
            "expected {bad:?} to be rejected, got: {err:?}"
        );
    }
}

/// A flush is held back until the section holds more than one time step past its first.
///
/// The section therefore stays open across the flush and the time step at 30 is included in it,
/// even though it has no associated value change.
#[test]
fn write_read_flush_below_time_step_gate() {
    let filename = "tests/flush_below_time_step_gate.fst";
    let reference_filename = "tests/flush_below_time_step_gate_fstapi.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(8),
            FstVarType::Reg,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    // only two time steps when the flush comes in, so it must not cut the section
    writer.time_change(10).unwrap();
    writer.signal_change(a, b"00000001").unwrap();
    writer.time_change(20).unwrap();
    writer.signal_change(a, b"00000010").unwrap();
    writer.flush().unwrap();
    // no value change follows this one, so it only survives in a section that stays open
    writer.time_change(30).unwrap();
    writer.finish().unwrap();

    //// the same history through the reference writer
    let mut reference = fstapi::Writer::create(reference_filename, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    let a = reference
        .create_var(
            fstapi::var_type::VCD_REG,
            fstapi::var_dir::OUTPUT,
            8,
            "a",
            None,
        )
        .unwrap();
    reference.emit_time_change(10).unwrap();
    reference.emit_value_change(a, b"00000001").unwrap();
    reference.emit_time_change(20).unwrap();
    reference.emit_value_change(a, b"00000010").unwrap();
    reference.flush();
    reference.emit_time_change(30).unwrap();
    drop(reference);

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [10, 20, 30]);
    let reference_wave = wellen::simple::read(reference_filename).unwrap();
    assert_eq!(
        wave.time_table(),
        reference_wave.time_table(),
        "the reference drops the flush request here, so nothing may be lost"
    );
}

/// A value change may follow a `flush` with no time change in between. The flush is only queued,
/// so the section stays open and the value attaches to its last time step; the cut happens at the
/// next time change — or, as here, never.
#[test]
fn write_read_value_after_flush() {
    let filename = "tests/value_after_flush.fst";
    let reference_filename = "tests/value_after_flush_fstapi.fst";
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(8),
            FstVarType::Reg,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    // a distinct value per step, so nothing here depends on how repeats are handled
    for (time, value) in [(10u64, b"00000001"), (20, b"00000011"), (30, b"00000111")] {
        writer.time_change(time).unwrap();
        writer.signal_change(a, value).unwrap();
    }
    // enough time steps for the reference to queue the flush
    writer.flush().unwrap();
    writer.signal_change(a, b"00000010").unwrap();
    writer.finish().unwrap();

    //// the same history through the reference writer
    let mut reference = fstapi::Writer::create(reference_filename, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    let ref_a = reference
        .create_var(
            fstapi::var_type::VCD_REG,
            fstapi::var_dir::OUTPUT,
            8,
            "a",
            None,
        )
        .unwrap();
    for (time, value) in [(10u64, b"00000001"), (20, b"00000011"), (30, b"00000111")] {
        reference.emit_time_change(time).unwrap();
        reference.emit_value_change(ref_a, value).unwrap();
    }
    reference.flush();
    reference.emit_value_change(ref_a, b"00000010").unwrap();
    drop(reference);

    //// read
    let wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), [10, 20, 30]);
    let reference_wave = wellen::simple::read(reference_filename).unwrap();
    assert_eq!(wave.time_table(), reference_wave.time_table());

    // the value written after the flush belongs to the last time step
    let read = |f: &str| {
        let mut reader = fstapi::Reader::open(f).unwrap();
        reader.set_mask_all();
        let mut changes = vec![];
        reader
            .for_each_block(|time, _handle, value, _var_len| {
                changes.push((time, String::from_utf8_lossy(value).to_string()))
            })
            .unwrap();
        changes
    };
    assert_eq!(
        *read(filename).last().unwrap(),
        (30, "00000010".to_string())
    );
    assert_eq!(read(filename), read(reference_filename));
}
