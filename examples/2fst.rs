// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>
//
// Small utility that reads in a VCD, GHW or FST file with wellen and then
// writes out the FST with the fst-writer library.
// Similar to vcd2fst, just that the input format does not have to be specified
// by the command name.

use clap::Parser;
use fst_writer::*;
use std::fs::File;
use std::io::BufWriter;
use wellen::*;

#[derive(Parser, Debug)]
#[command(name = "2fst")]
#[command(author = "Kevin Laeufer <laeufer@cornell.edu>")]
#[command(version)]
#[command(about = "Converts a VCD, GHW or FST file to an FST file.", long_about = None)]
struct Args {
    #[arg(value_name = "INPUT", index = 1)]
    input: std::path::PathBuf,
    #[arg(value_name = "FSTFILE", index = 2)]
    fst_file: std::path::PathBuf,
    #[arg(long, help = "")]
    start_time: Option<String>,
    #[arg(long, help = "")]
    end_time: Option<String>,
}

// write a value change block when we reach 128 MiB of in memory data
const FLUSH_AT: usize = 128 * 1024 * 1024;

fn main() {
    let args = Args::parse();
    let start_time = args.start_time.as_ref().map(|s| parse_time(&s));
    let end_time = args.end_time.as_ref().map(|s| parse_time(&s));
    let (mut out, signal_ref_map, filter_start, filter_end) =
        write_header(args.input.clone(), args.fst_file, start_time, end_time);
    let load_opts = LoadOptions::default();

    // stream all signals into the output fst
    let mut wave = stream::read_from_file(args.input, &load_opts)
        .expect("failed to read input in streaming mode");

    // apply time filters
    let mut filter = stream::Filter::all();
    filter.start = filter_start;
    filter.end = Some(filter_end);
    let mut prev_time: Option<Time> = None;
    wave.stream_changes::<()>(filter, |time, signal, value| {
        // we need to manually filter time in order to work around a bug in wellen:
        // https://github.com/ekiwi/wellen/issues/141
        if time >= filter_start && time <= filter_end {
            // emit time change
            if prev_time.is_none_or(|prev| prev < time) {
                out.time_change(time).expect("failed time change");
                prev_time = Some(time);
            }

            // emit change
            let fst_id = signal_ref_map[&signal];
            match value {
                SignalValueRef::Event => {
                    out.signal_change(fst_id, &[])
                        .expect("failed to write value change");
                }
                SignalValueRef::BitVec(bv) => {
                    out.signal_change(fst_id, bv.bit_string().as_bytes())
                        .expect("failed to write value change");
                }
                SignalValueRef::String(_value) => {
                    todo!("deal with var len string");
                }
                SignalValueRef::Real(_value) => {
                    todo!("deal with real value: {value}");
                }
            }

            // flush buffer
            if out.size() >= FLUSH_AT {
                out.flush().expect("failed to flush buffer");
            }
        }

        Ok(())
    })
    .expect("failed to parse signal changes");

    out.finish().expect("failed to finish writing the FST file");
}

#[derive(Debug, Copy, Clone)]
struct TimeWithUnit {
    time: u64,
    unit: TimescaleUnit,
}

fn parse_time(value: &str) -> TimeWithUnit {
    if let Some(time) = value.strip_suffix("zs") {
        TimeWithUnit {
            time: time.trim().parse().unwrap(),
            unit: TimescaleUnit::ZeptoSeconds,
        }
    } else if let Some(time) = value.strip_suffix("as") {
        TimeWithUnit {
            time: time.trim().parse().unwrap(),
            unit: TimescaleUnit::AttoSeconds,
        }
    } else if let Some(time) = value.strip_suffix("fs") {
        TimeWithUnit {
            time: time.trim().parse().unwrap(),
            unit: TimescaleUnit::FemtoSeconds,
        }
    } else if let Some(time) = value.strip_suffix("ps") {
        TimeWithUnit {
            time: time.trim().parse().unwrap(),
            unit: TimescaleUnit::PicoSeconds,
        }
    } else if let Some(time) = value.strip_suffix("ns") {
        TimeWithUnit {
            time: time.trim().parse().unwrap(),
            unit: TimescaleUnit::NanoSeconds,
        }
    } else if let Some(time) = value.strip_suffix("us") {
        TimeWithUnit {
            time: time.trim().parse().unwrap(),
            unit: TimescaleUnit::MicroSeconds,
        }
    } else if let Some(time) = value.strip_suffix("ms") {
        TimeWithUnit {
            time: time.trim().parse().unwrap(),
            unit: TimescaleUnit::MilliSeconds,
        }
    } else if let Some(time) = value.strip_suffix("s") {
        TimeWithUnit {
            time: time.trim().parse().unwrap(),
            unit: TimescaleUnit::Seconds,
        }
    } else {
        panic!("Cannot parse time `{value}`. A valid time unit is required.")
    }
}

impl TimeWithUnit {
    fn as_time(&self, mut timescale_exponent: i8) -> Time {
        let mut time = self.time as Time;
        timescale_exponent = self.unit.to_exponent().unwrap_or(0) - timescale_exponent;
        while timescale_exponent < 0 {
            time /= 10;
            timescale_exponent += 1;
        }
        while timescale_exponent > 0 {
            time *= 10;
            timescale_exponent -= 1;
        }
        time
    }
}

fn time_scale_exp(timescale: Option<Timescale>) -> i8 {
    let mut timescale_exponent = timescale.and_then(|x| x.unit.to_exponent()).unwrap_or(0);
    let mut factor = timescale.map_or(1, |x| x.factor);

    if factor == 0 {
        println!("Error: timescale factor is zero, setting it to 1");
        factor = 1;
    }

    while factor % 10 == 0 {
        factor /= 10;
        timescale_exponent += 1;
    }
    timescale_exponent
}

fn write_header<P: AsRef<std::path::Path>>(
    filename: P,
    fst_file: impl AsRef<std::path::Path>,
    filter_start_time: Option<TimeWithUnit>,
    filter_end_time: Option<TimeWithUnit>,
) -> (FstBodyWriter<BufWriter<File>>, SignalRefMap, Time, Time) {
    // TODO: once wellen supports start/end time access in streaming mode, it will be enough to open
    //       the file just once!
    let wave = simple::read(filename).expect("failed to read input");
    let timescale_exponent = time_scale_exp(wave.hierarchy().timescale());
    let orig_start_time = *wave.time_table().first().unwrap();
    let orig_end_time = *wave.time_table().last().unwrap();
    let start_time: Time = filter_start_time
        .map(|f| std::cmp::max(f.as_time(timescale_exponent), orig_start_time))
        .unwrap_or(orig_start_time);
    let end_time: Time = filter_end_time
        .map(|f| std::cmp::min(f.as_time(timescale_exponent), orig_end_time))
        .unwrap_or(orig_end_time);
    let info = FstInfo {
        start_time,
        timescale_exponent,
        version: wave.hierarchy().version().to_string(),
        date: wave.hierarchy().date().to_string(),
        file_type: FstFileType::Verilog, // TODO
    };
    let mut out = open_fst(fst_file, &info).expect("failed to open output");
    let signal_ref_map = write_hierarchy(wave.hierarchy(), &mut out);
    let out = out
        .finish()
        .expect("failed to write FST header or hierarchy");
    (out, signal_ref_map, start_time, end_time)
}

type SignalRefMap = std::collections::HashMap<SignalRef, FstSignalId>;

fn write_hierarchy<W: std::io::Write + std::io::Seek>(
    hier: &Hierarchy,
    out: &mut FstHeaderWriter<W>,
) -> SignalRefMap {
    let mut signal_ref_map = SignalRefMap::new();
    for item in hier.items() {
        match item {
            ItemRef::Scope(scope) => write_scope(hier, out, &mut signal_ref_map, scope),
            ItemRef::Var(var) => write_var(hier, out, &mut signal_ref_map, var),
        }
    }
    signal_ref_map
}

fn write_scope<W: std::io::Write + std::io::Seek>(
    hier: &Hierarchy,
    out: &mut FstHeaderWriter<W>,
    signal_ref_map: &mut SignalRefMap,
    scope: ScopeRef,
) {
    let scope = &hier[scope];
    let name = scope.name(hier);
    let component = scope.component(hier).unwrap_or("");
    let tpe = match scope.scope_type() {
        ScopeType::Module => FstScopeType::Module,
        ScopeType::Task => todo!(),
        ScopeType::Function => todo!(),
        ScopeType::Begin => todo!(),
        ScopeType::Fork => todo!(),
        ScopeType::Generate => todo!(),
        ScopeType::Struct => todo!(),
        ScopeType::Union => todo!(),
        ScopeType::Class => todo!(),
        ScopeType::Interface => todo!(),
        ScopeType::Package => todo!(),
        ScopeType::Program => todo!(),
        ScopeType::VhdlArchitecture => todo!(),
        ScopeType::VhdlProcedure => todo!(),
        ScopeType::VhdlFunction => todo!(),
        ScopeType::VhdlRecord => todo!(),
        ScopeType::VhdlProcess => todo!(),
        ScopeType::VhdlBlock => todo!(),
        ScopeType::VhdlForGenerate => todo!(),
        ScopeType::VhdlIfGenerate => todo!(),
        ScopeType::VhdlGenerate => todo!(),
        ScopeType::VhdlPackage => todo!(),
        ScopeType::GhwGeneric => todo!(),
        ScopeType::VhdlArray => todo!(),
        _ => todo!(),
    };
    out.scope(name, component, tpe)
        .expect("failed to write scope");

    for item in scope.items(hier) {
        match item {
            ItemRef::Scope(scope) => write_scope(hier, out, signal_ref_map, scope),
            ItemRef::Var(var) => write_var(hier, out, signal_ref_map, var),
        }
    }
    out.up_scope().expect("failed to close scope");
}

fn write_var<W: std::io::Write + std::io::Seek>(
    hier: &Hierarchy,
    out: &mut FstHeaderWriter<W>,
    signal_ref_map: &mut SignalRefMap,
    var: VarRef,
) {
    let var = &hier[var];
    let name = var.name(hier);
    let signal_tpe = match var.signal_encoding(hier) {
        SignalEncoding::String => todo!("support varlen!"),
        SignalEncoding::Real => FstSignalType::real(),
        SignalEncoding::BitVector(len) => FstSignalType::bit_vec(len),
    };
    let tpe = match var.var_type() {
        VarType::Event => FstVarType::Event,
        VarType::Integer => FstVarType::Integer,
        VarType::Parameter => FstVarType::Parameter,
        VarType::Real => FstVarType::Real,
        VarType::Reg => FstVarType::Reg,
        VarType::Supply0 => FstVarType::Supply0,
        VarType::Supply1 => FstVarType::Supply1,
        VarType::Time => FstVarType::Time,
        VarType::Tri => FstVarType::Tri,
        VarType::TriAnd => FstVarType::TriAnd,
        VarType::TriOr => FstVarType::TriOr,
        VarType::TriReg => FstVarType::TriReg,
        VarType::Tri0 => FstVarType::Tri0,
        VarType::Tri1 => FstVarType::Tri1,
        VarType::WAnd => FstVarType::Wand,
        VarType::Wire => FstVarType::Wire,
        VarType::WOr => FstVarType::Wor,
        VarType::String => FstVarType::GenericString,
        VarType::Port => FstVarType::Port,
        VarType::SparseArray => FstVarType::SparseArray,
        VarType::RealTime => FstVarType::RealTime,
        VarType::Bit => FstVarType::Bit,
        VarType::Logic => FstVarType::Logic,
        VarType::Int => FstVarType::Int,
        VarType::ShortInt => FstVarType::ShortInt,
        VarType::LongInt => FstVarType::LongInt,
        VarType::Byte => FstVarType::Byte,
        VarType::Enum => FstVarType::Enum,
        VarType::ShortReal => FstVarType::ShortReal,
        VarType::Boolean => todo!(),
        VarType::BitVector => todo!(),
        VarType::StdLogic => todo!(),
        VarType::StdLogicVector => todo!(),
        VarType::StdULogic => todo!(),
        VarType::StdULogicVector => todo!(),
        VarType::RealParameter => todo!(),
        VarType::EventParameter => todo!(),
    };
    let dir = match var.direction() {
        VarDirection::Unknown => FstVarDirection::Implicit,
        VarDirection::Implicit => FstVarDirection::Implicit,
        VarDirection::Input => FstVarDirection::Input,
        VarDirection::Output => FstVarDirection::Output,
        VarDirection::InOut => FstVarDirection::InOut,
        VarDirection::Buffer => FstVarDirection::Buffer,
        VarDirection::Linkage => FstVarDirection::Linkage,
    };

    let alias = signal_ref_map.get(&var.signal_ref()).cloned();
    let fst_signal_id = out
        .var(name, signal_tpe, tpe, dir, alias)
        .expect("failed to write variable");
    if alias.is_none() {
        signal_ref_map.insert(var.signal_ref(), fst_signal_id);
    }
}
