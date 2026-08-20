// read files written by fst-writer with the reference implementation (GTKWave's fstapi),
// in order to check the things that the wellen reader does not look at

use fst_writer::*;

/// The reference starts the first value change section, and thus the file, at the first time
/// change, but only if no value was written before it. A value written earlier pulls the start
/// back to 0 instead.
#[test]
fn fstapi_read_first_time_change_not_zero() {
    let filename = "tests/first_time_change_fstapi.fst";
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

    //// read with the reference implementation
    let mut reader = fstapi::Reader::open(filename).unwrap();
    assert_eq!(reader.end_time(), 20);
    // the header start time is not read by wellen, only by the reference implementation
    assert_eq!(reader.start_time(), 10);

    // collect all value changes; the reference reader reports the frame at the section start time,
    // thus we only look at the earliest time and not at the exact sequence
    reader.set_mask_all();
    let mut times = vec![];
    reader
        .for_each_block(|time, _handle, value, _var_len| {
            times.push((time, String::from_utf8_lossy(value).to_string()))
        })
        .unwrap();
    let first_time = times.iter().map(|(t, _)| *t).min();
    assert_eq!(first_time, Some(10), "no sample before time 10: {times:?}");
}

/// Writes the same signal history with the reference implementation and with `fst-writer` and
/// compares what the reference reader makes of both files.
#[test]
fn fstapi_diff_first_time_change() {
    let history = [
        Step::Time(10),
        Step::Value(b"00000001"),
        Step::Time(20),
        Step::Value(b"00000000"),
        Step::Time(30),
        Step::Value(b"00001111"),
    ];

    let fstapi_file = "tests/first_time_change_diff_fstapi.fst";
    let our_file = "tests/first_time_change_diff.fst";
    write_with_fstapi(fstapi_file, 8, &history);
    write_with_fst_writer(our_file, 8, &history);

    //// both files need to look the same to the reference reader
    assert_eq!(read_with_fstapi(our_file), read_with_fstapi(fstapi_file));
}

/// Returns the start time, the end time and all value changes, as seen by the reference reader.
fn read_with_fstapi(filename: &str) -> (u64, u64, Vec<(u64, String)>) {
    let mut reader = fstapi::Reader::open(filename).unwrap();
    reader.set_mask_all();
    let mut changes = vec![];
    reader
        .for_each_block(|time, _handle, value, _var_len| {
            changes.push((time, String::from_utf8_lossy(value).to_string()))
        })
        .unwrap();
    (reader.start_time(), reader.end_time(), changes)
}

/// A history that leaves the section without any value change produces a file with no value change
/// section at all, in both writers: the reference never finalizes a section that holds nothing.
/// Neither file is usable — readers reject a file that has no value change section — but they have
/// to be *equally* unusable, which is what this pins.
///
/// Our file is the better formed of the two: the reference leaves its unfinalized section header
/// behind as an empty skip block, which readers cannot even walk past, whereas we write nothing at
/// all.
#[test]
fn fstapi_diff_no_value_changes() {
    for (name, history) in [
        // a value that only ever reaches the frame, and a first time change at 0 that makes
        // readers skip that frame
        (
            "value_then_time_change",
            &[Step::Value(b"1"), Step::Time(0)][..],
        ),
        // time changes without a single value
        ("only_time_changes", &[Step::Time(0), Step::Time(5)][..]),
    ] {
        let fstapi_file = format!("tests/no_vc_{name}_fstapi.fst");
        let our_file = format!("tests/no_vc_{name}.fst");
        write_with_fstapi(&fstapi_file, 1, history);
        write_with_fst_writer(&our_file, 1, history);

        // `Reader::open` fails for both, and it has to fail the same way
        let outcome = |f: &str| {
            fstapi::Reader::open(f)
                .map(|_| ())
                .map_err(|e| format!("{e:?}"))
        };
        assert_eq!(
            outcome(&our_file),
            outcome(&fstapi_file),
            "{name}: the reference reader must make the same thing of both files"
        );
    }
}

/// One call of the writer API, so the same history can be replayed into both writers.
enum Step<'a> {
    Time(u64),
    Value(&'a [u8]),
}

fn write_with_fstapi(filename: &str, width: u32, history: &[Step]) {
    let mut writer = fstapi::Writer::create(filename, true)
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();
    let var = writer
        .create_var(
            fstapi::var_type::VCD_REG,
            fstapi::var_dir::OUTPUT,
            width,
            "a",
            None,
        )
        .unwrap();
    for step in history {
        match step {
            Step::Time(time) => writer.emit_time_change(*time).unwrap(),
            Step::Value(value) => writer.emit_value_change(var, value).unwrap(),
        }
    }
}

fn write_with_fst_writer(filename: &str, width: u32, history: &[Step]) {
    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "test 0.2.3".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let mut writer = open_fst(filename, &info).unwrap();
    let var = writer
        .var(
            "a",
            FstSignalType::bit_vec(width),
            FstVarType::Logic,
            FstVarDirection::Output,
            None,
        )
        .unwrap();
    let mut writer = writer.finish().unwrap();
    for step in history {
        match step {
            Step::Time(time) => writer.time_change(*time).unwrap(),
            Step::Value(value) => writer.signal_change(var, value).unwrap(),
        }
    }
    writer.finish().unwrap();
}
