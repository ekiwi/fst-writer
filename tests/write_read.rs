// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>
//
// write FST files with fst-writer and read them again with the wellen library
// (using fst-native as the backend)

use fst_reader::{FstFilter, FstReader, FstSignalValue};
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

#[test]
fn write_read_simple_in_memory() {
    let version = "test 0.2.3";
    let date = "2034-10-10";

    ///////// write to an in-memory buffer instead of a file
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: version.to_string(),
        date: date.to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = new_in_memory(&info).unwrap();
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
    writer.up_scope().unwrap();

    let mut writer = writer.finish().unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.time_change(1).unwrap();
    writer.signal_change(a, b"1").unwrap();

    let buf = writer.finish().unwrap();
    let bytes = buf.into_inner();
    assert!(!bytes.is_empty());

    //// read back the in-memory bytes
    let mut wave = wellen::simple::read_from_reader(std::io::Cursor::new(bytes)).unwrap();

    assert_eq!(wave.time_table(), [0, 1]);
    assert_eq!(wave.hierarchy().date(), date);
    assert_eq!(wave.hierarchy().version(), version);

    let a_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[a_ref]);
    let signal_a = wave.get_signal(a_ref).unwrap();
    assert_eq!(
        signal_values_to_string(signal_a, wave.time_table()),
        "(0: 0), (1: 1)"
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

/// Writes three value changes at times 10, 20 and 30, where the one at 20 repeats the value the
/// signal already holds, and returns the raw value change records that come back out.
///
/// `wellen` collapses the repeat again on read, so this decodes the records with `fst-reader`
/// instead.
fn write_read_repeated_value_changes(filename: &str, deduplicate: bool) -> Vec<(u64, String)> {
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
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    writer.set_deduplicate(deduplicate);

    writer.time_change(10).unwrap();
    writer.signal_change(a, b"1").unwrap();
    writer.time_change(20).unwrap();
    writer.signal_change(a, b"1").unwrap(); // the same value again
    writer.time_change(30).unwrap();
    writer.signal_change(a, b"0").unwrap();
    writer.finish().unwrap();

    let file = std::io::BufReader::new(std::fs::File::open(filename).unwrap());
    let mut reader = FstReader::open(file).unwrap();
    let mut changes = vec![];
    reader
        .read_signals(&FstFilter::all(), |time, _handle, value| {
            // whatever the section start contributes at time 0 is not what this test is about
            if time >= 10 {
                let value = match value {
                    FstSignalValue::String(value) => String::from_utf8_lossy(value).to_string(),
                    FstSignalValue::Real(value) => format!("{value:?}"),
                };
                changes.push((time, value));
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    changes
}

/// Without deduplication, a value change that repeats the value a signal already holds is still
/// recorded, like the reference does with `FST_REMOVE_DUPLICATE_VC` disabled.
#[test]
fn write_read_repeated_value() {
    let changes = write_read_repeated_value_changes("tests/repeated_value.fst", false);
    assert_eq!(
        changes,
        [
            (10, "1".to_string()),
            (20, "1".to_string()),
            (30, "0".to_string())
        ],
        "the repeated value change has to survive"
    );
}

/// With deduplication, a value change that repeats the value a signal already holds is dropped.
#[test]
fn write_read_repeated_value_deduplicated() {
    let changes = write_read_repeated_value_changes("tests/repeated_value_dedup.fst", true);
    assert_eq!(
        changes,
        [(10, "1".to_string()), (30, "0".to_string())],
        "the repeated value change has to be dropped"
    );
}
