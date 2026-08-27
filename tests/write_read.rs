// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>
//
// write FST files with fst-writer and read them again with the wellen library
// (using fst-native as the backend)

use fst_reader::{FstFilter, FstReader, FstSignalValue};
use fst_writer::*;
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
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

fn test_info(version: &str) -> FstInfo {
    FstInfo {
        start_time: 0,
        timescale_exponent: 0,
        version: version.to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    }
}

/// `expand_special_vector_cases` fills a value that is shorter than its signal, repeating a
/// leading `x`/`z` and zero-extending a leading `0`/`1`.
#[test]
fn write_read_short_values_are_extended() {
    let filename = "tests/short_values.fst";
    let info = test_info("test 0.2.3");
    let mut writer = open_fst(filename, &info).unwrap();
    let v = writer
        .var(
            "v",
            FstSignalType::bit_vec(8),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();

    writer.time_change(0).unwrap();
    writer.signal_change(v, b"1").unwrap(); // zero extended
    writer.time_change(1).unwrap();
    writer.signal_change(v, b"x1").unwrap(); // x extended
    writer.time_change(2).unwrap();
    writer.signal_change(v, b"Z0").unwrap(); // Z extended
    writer.time_change(3).unwrap();
    writer.signal_change(v, b"01010101").unwrap(); // exact length, no extension
    writer.finish().unwrap();

    let mut wave = wellen::simple::read(filename).unwrap();
    let v_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[v_ref]);
    assert_eq!(
        signal_values_to_string(wave.get_signal(v_ref).unwrap(), wave.time_table()),
        "(0: 00000001), (1: xxxxxxx1), (2: zzzzzzz0), (3: 01010101)"
    );
}

/// Every character `encode_9_value` accepts must survive a round trip on a one bit signal.
#[test]
fn write_read_all_nine_state_characters() {
    let filename = "tests/nine_state.fst";
    let info = test_info("test 0.2.3");
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

    // upper and lower case of each of the nine `std_logic` states
    let values: &[u8] = b"01xXzZhHuUwWlL-";
    for (ii, value) in values.iter().enumerate() {
        writer.time_change(ii as u64).unwrap();
        writer.signal_change(a, &[*value]).unwrap();
    }
    writer.finish().unwrap();

    let mut wave = wellen::simple::read(filename).unwrap();
    let a_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[a_ref]);
    let actual = signal_values_to_string(wave.get_signal(a_ref).unwrap(), wave.time_table());
    // both cases of a state encode to the same value, so the upper case repeat of each is not
    // recorded as a change
    assert_eq!(
        actual,
        "(0: 0), (1: 1), (2: x), (4: z), (6: h), (8: u), (10: w), (12: l), (14: -)"
    );
}

/// Time must never go backwards.
#[test]
fn write_decreasing_time_is_an_error() {
    let filename = "tests/time_decrease.fst";
    let info = test_info("test 0.2.3");
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

    writer.time_change(10).unwrap();
    let err = writer.time_change(9).unwrap_err();
    assert!(
        matches!(err, FstWriteError::TimeDecrease(10, 9)),
        "unexpected error: {err:?}"
    );
    // the same time is still fine
    writer.time_change(10).unwrap();
}

/// The header holds the version string in a fixed size field.
#[test]
fn write_version_string_too_long() {
    let filename = "tests/version_too_long.fst";
    let version = "v".repeat(128);
    match open_fst(filename, &test_info(&version)) {
        Err(FstWriteError::StringTooLong(max_len, value)) => {
            assert_eq!(max_len, 128);
            assert_eq!(value, version);
        }
        Err(other) => panic!("unexpected error: {other:?}"),
        Ok(_) => panic!("expected the over long version string to be rejected"),
    }
}

/// `size` reports the buffered bytes, which grow with the value changes and drop on a flush.
#[test]
fn write_reported_buffer_size() {
    let filename = "tests/buffer_size.fst";
    let info = test_info("test 0.2.3");
    let mut writer = open_fst(filename, &info).unwrap();
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(64),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();

    let empty = writer.size();
    writer.time_change(0).unwrap();
    writer.signal_change(a, b"1").unwrap();
    for time in 1..8u64 {
        writer.time_change(time).unwrap();
        writer.signal_change(a, b"0").unwrap();
    }
    let filled = writer.size();
    assert!(
        filled > empty,
        "the buffer should have grown: {empty} -> {filled}"
    );

    // a flush empties the value change lists again
    writer.flush().unwrap();
    writer.time_change(8).unwrap();
    let flushed = writer.size();
    assert!(
        flushed < filled,
        "the flush should have released the buffered changes: {filled} -> {flushed}"
    );

    writer.finish().unwrap();
    drop(wellen::simple::read(filename).unwrap());
}

/// Enough data to make both the time table and the value change streams worth compressing - and,
/// for the incompressible signal, to make the writer fall back to the raw bytes.
#[test]
fn write_read_compressed_and_incompressible_sections() {
    let filename = "tests/compression.fst";
    let info = test_info("test 0.2.3");
    let mut writer = open_fst(filename, &info).unwrap();
    // toggles between two values: highly compressible
    let a = writer
        .var(
            "a",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    // 256 pseudo random bits per change: lz4 cannot do anything with it
    let noise = writer
        .var(
            "noise",
            FstSignalType::bit_vec(256),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    // written last, so that its offset in the section is far enough behind the noise to need a
    // multi byte offset delta
    let tail = writer
        .var(
            "tail",
            FstSignalType::bit_vec(1),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();

    let mut rng = SmallRng::seed_from_u64(0xec6dbd474480bf77);
    let mut next_noise = || {
        (0..256)
            .map(|_| if rng.random() { b'1' } else { b'0' })
            .collect::<Vec<u8>>()
    };

    let steps = 200u64;
    let mut expected_noise = Vec::with_capacity(steps as usize);
    for time in 0..steps {
        writer.time_change(time).unwrap();
        writer
            .signal_change(a, if time % 2 == 0 { b"0" } else { b"1" })
            .unwrap();
        let value = next_noise();
        writer.signal_change(noise, &value).unwrap();
        expected_noise.push(String::from_utf8(value).unwrap());
        writer.signal_change(tail, b"1").unwrap();
    }
    writer.finish().unwrap();

    let mut wave = wellen::simple::read(filename).unwrap();
    assert_eq!(wave.time_table(), (0..steps).collect::<Vec<_>>());

    let (a_ref, noise_ref) = (
        SignalRef::from_index(0).unwrap(),
        SignalRef::from_index(1).unwrap(),
    );
    wave.load_signals(&[a_ref, noise_ref]);

    let signal_a = wave.get_signal(a_ref).unwrap();
    let a_values = signal_a
        .iter_changes()
        .map(|(_, v)| v.to_bit_string().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(a_values.len(), steps as usize);
    assert!(
        a_values
            .iter()
            .enumerate()
            .all(|(ii, v)| v == if ii % 2 == 0 { "0" } else { "1" })
    );

    // the trailing signal only ever changes once, at time zero
    let tail_ref = SignalRef::from_index(2).unwrap();
    wave.load_signals(&[tail_ref]);
    assert_eq!(
        signal_values_to_string(wave.get_signal(tail_ref).unwrap(), wave.time_table()),
        "(0: 1)"
    );

    let signal_noise = wave.get_signal(noise_ref).unwrap();
    let noise_values = signal_noise
        .iter_changes()
        .map(|(_, v)| v.to_bit_string().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(noise_values, expected_noise);
}

/// An all digital vector whose width is not a multiple of eight leaves a partially filled byte
/// that still has to be written out.
#[test]
fn write_read_digital_vector_with_partial_byte() {
    let filename = "tests/partial_byte.fst";
    let info = test_info("test 0.2.3");
    let mut writer = open_fst(filename, &info).unwrap();
    // 12 bits: one full byte plus half of a second one
    let v = writer
        .var(
            "v",
            FstSignalType::bit_vec(12),
            FstVarType::Logic,
            FstVarDirection::Implicit,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();

    writer.time_change(0).unwrap();
    writer.signal_change(v, b"110010101111").unwrap();
    writer.time_change(1).unwrap();
    writer.signal_change(v, b"000000000001").unwrap();
    writer.finish().unwrap();

    let mut wave = wellen::simple::read(filename).unwrap();
    let v_ref = SignalRef::from_index(0).unwrap();
    wave.load_signals(&[v_ref]);
    assert_eq!(
        signal_values_to_string(wave.get_signal(v_ref).unwrap(), wave.time_table()),
        "(0: 110010101111), (1: 000000000001)"
    );
}
