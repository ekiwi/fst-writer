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
}

fn main() {
    let args = Args::parse();

    let mut wave = simple::read(args.input).expect("failed to read input");

    let mut timescale_exponent = wave
        .hierarchy()
        .timescale()
        .and_then(|x| x.unit.to_exponent())
        .unwrap_or(0);
    let mut factor = wave.hierarchy().timescale().map_or(1, |x| x.factor);

    if factor == 0 {
        println!("Error: timescale factor is zero, setting it to 1");
        factor = 1;
    }

    while factor % 10 == 0 {
        factor /= 10;
        timescale_exponent += 1;
    }

    let info = FstInfo {
        start_time: wave.time_table()[0],
        timescale_exponent,
        version: wave.hierarchy().version().to_string(),
        date: wave.hierarchy().date().to_string(),
        file_type: FstFileType::Verilog, // TODO
    };
    let mut out = open_fst(args.fst_file, &info).expect("failed to open output");
    let signal_ref_map = write_hierarchy(wave.hierarchy(), &mut out);
    let mut out = out
        .finish()
        .expect("failed to write FST header or hierarchy");

    // load all signals into memory
    let all_signals: Vec<_> = signal_ref_map.keys().cloned().collect();
    wave.load_signals_multi_threaded(&all_signals);
    write_value_changes(&wave, &mut out, &signal_ref_map, factor);
    out.finish().expect("failed to finish writing the FST file");
}

/// Writes all value changes from the source file to the FST.
/// Note this is not the most efficient way to do this!
/// A faster version would write each signal directly to the FST instead
/// of writing changes based on the time step.
fn write_value_changes<W: std::io::Write + std::io::Seek>(
    wave: &simple::Waveform,
    out: &mut FstBodyWriter<W>,
    signal_ref_map: &SignalRefMap,
    factor: u32,
) {
    // sort signal ids in order to get a deterministic output
    let mut signal_ids: Vec<_> = signal_ref_map.iter().map(|(a, b)| (*a, *b)).collect();
    signal_ids.sort_by_key(|(wellen_id, _)| *wellen_id);

    // signal data iterators
    let mut signals: Vec<_> = signal_ids
        .iter()
        .map(|(wellen_ref, _)| {
            wave.get_signal(*wellen_ref)
                .expect("failed to find signal")
                .iter_changes()
                .peekable()
        })
        .collect();

    // extract out the fst ids for convenience
    let fst_ids: Vec<_> = signal_ids.into_iter().map(|(_, fst_id)| fst_id).collect();

    for (time_idx, time) in wave.time_table().iter().enumerate() {
        let time_idx = time_idx as TimeTableIdx;
        out.time_change(*time * factor as u64)
            .expect("failed time change");
        for (signal, fst_id) in signals.iter_mut().zip(fst_ids.iter()) {
            // while there is a change at the current time step
            while signal
                .peek()
                .map(|(change_idx, _)| *change_idx == time_idx)
                .unwrap_or(false)
            {
                // consume change
                let (_, value) = signal.next().unwrap();
                if let Some(bit_str) = value.to_bit_string() {
                    out.signal_change(*fst_id, bit_str.as_bytes())
                        .expect("failed to write value change");
                } else if let SignalValue::Real(value) = value {
                    todo!("deal with real value: {value}");
                } else {
                    todo!("deal with var len string");
                }
            }
        }
    }
}

struct SignalTracker {
    /// the value of `time_indices[index]`, None if no more changes are available
    next_change: Option<TimeTableIdx>,
    /// index into the `time_indices` of the signal
    index: u32,
}

type SignalRefMap = std::collections::HashMap<SignalRef, FstSignalId>;

fn write_hierarchy<W: std::io::Write + std::io::Seek>(
    hier: &Hierarchy,
    out: &mut FstHeaderWriter<W>,
) -> SignalRefMap {
    let mut signal_ref_map = SignalRefMap::new();
    for item in hier.items() {
        match item {
            HierarchyItem::Scope(scope) => write_scope(hier, out, &mut signal_ref_map, scope),
            HierarchyItem::Var(var) => write_var(hier, out, &mut signal_ref_map, var),
        }
    }
    signal_ref_map
}

fn write_scope<W: std::io::Write + std::io::Seek>(
    hier: &Hierarchy,
    out: &mut FstHeaderWriter<W>,
    signal_ref_map: &mut SignalRefMap,
    scope: &Scope,
) {
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
    };
    out.scope(name, component, tpe)
        .expect("failed to write scope");

    for item in scope.items(hier) {
        match item {
            HierarchyItem::Scope(scope) => write_scope(hier, out, signal_ref_map, scope),
            HierarchyItem::Var(var) => write_var(hier, out, signal_ref_map, var),
        }
    }
    out.up_scope().expect("failed to close scope");
}

fn write_var<W: std::io::Write + std::io::Seek>(
    hier: &Hierarchy,
    out: &mut FstHeaderWriter<W>,
    signal_ref_map: &mut SignalRefMap,
    var: &Var,
) {
    let name = var.name(hier);
    let signal_tpe = match var.signal_encoding() {
        SignalEncoding::String => todo!("support varlen!"),
        SignalEncoding::Real => FstSignalType::real(),
        SignalEncoding::BitVector(len) => FstSignalType::bit_vec(len.get()),
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
