#![no_main]

use fst_writer::{open_fst, FstFileType, FstInfo, FstSignalType, FstVarDirection, FstVarType};
use fstapi::{var_dir, var_type, Writer};
use libfuzzer_sys::fuzz_target;
use tempfile::tempdir;
use wellen::GetItem;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }

    let outdir = tempdir().unwrap();

    // Create the waveform.
    let fstfile = outdir.path().join("fstapi.fst");
    let mut fstapi = Writer::create(&fstfile, true)
        .unwrap()
        .comment("FST waveform example")
        .unwrap()
        .timescale_from_str("1ns")
        .unwrap();

    let info = FstInfo {
        start_time: 0,
        timescale_exponent: -9,
        version: "0.0.0".to_string(),
        date: "2034-10-10".to_string(),
        file_type: FstFileType::Verilog,
    };
    let writerfile = outdir.path().join("fst-writer.fst");
    let mut writer = open_fst(&writerfile, &info).unwrap();

    let vars = (0..(data[0] as usize).min(1))
        .map(|i| {
            let name = &format!("s{}", i);
            let width = 8;
            (
                fstapi
                    .create_var(var_type::VCD_REG, var_dir::OUTPUT, width, name, None)
                    .unwrap(),
                writer
                    .var(
                        name,
                        FstSignalType::bit_vec(8),
                        FstVarType::Logic,
                        FstVarDirection::Output,
                        None,
                    )
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();

    if vars.is_empty() {
        return;
    }

    let mut writer = writer.finish().unwrap();

    let mut timestamp: u64 = 0;

    fstapi.emit_time_change(0).unwrap();
    writer.time_change(0).unwrap();

    for chunk in data[1..].chunks(3) {
        let &[dt, signal, value] = chunk else {
            break;
        };

        timestamp += dt.min(1) as u64;
        let signal = vars[signal as usize % vars.len()];
        let value = format!("{:08b}", value);

        println!("{} {} {}", timestamp, signal.0, value);

        fstapi.emit_time_change(timestamp).unwrap();
        fstapi
            .emit_value_change(signal.0, value.as_bytes())
            .unwrap();

        writer.time_change(timestamp).unwrap();
        writer.signal_change(signal.1, value.as_bytes()).unwrap();
    }

    writer.finish().unwrap();

    drop(fstapi);

    println!("Files: {:?} {:?}", fstfile, writerfile);

    // read
    let fstwave = wellen::simple::read(fstfile).unwrap();
    let writerwave = wellen::simple::read(writerfile).unwrap();

    // Some times the timetable differ in how the times are being duplicated.
    fn dedup<T: Clone + PartialEq>(x: &[T]) -> Vec<T> {
        let mut x = x.to_vec();
        x.dedup();
        x
    }

    assert_eq!(dedup(fstwave.time_table()), dedup(writerwave.time_table()));
    let vars = |wave: &wellen::simple::Waveform| {
        let h = wave.hierarchy();
        h.vars()
            .map(|r| h.get(r))
            .map(|v| v.full_name(h))
            .collect::<Vec<_>>()
    };

    for (fstvar, writervar) in vars(&fstwave)
        .into_iter()
        .zip(vars(&writerwave).into_iter())
    {
        assert_eq!(fstvar, writervar);
    }
});
