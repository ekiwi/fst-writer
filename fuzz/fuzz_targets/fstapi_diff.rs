#![no_main]

use arbitrary::Arbitrary;
use fst_reader::{
    FstFilter, FstHierarchyEntry, FstReader, FstSignalValue, ReadSignalsError, ReaderError,
};
use fst_writer::{
    open_fst, FstFileType, FstInfo, FstScopeType, FstSignalId, FstSignalType, FstVarDirection,
    FstVarType,
};
use fstapi::{var_dir, var_type};
use libfuzzer_sys::fuzz_target;
use std::io::BufReader;
use std::path::Path;
use tempfile::tempdir;

/// A signal type both writers can describe.
///
/// Zero width (variable length) signals are deliberately absent: `fst-writer` only accepts an
/// empty value for those, and the section it then writes has no length prefix, which sends the
/// reference reader off into uninitialized memory.
#[derive(Debug, Clone, Copy, Arbitrary)]
enum VarType {
    /// `bits + 1` wide, so never zero
    BitVec(u8),
    Real,
}

#[derive(Debug, Arbitrary)]
enum Item {
    Scope {
        name: String,
        component: String,
        tpe: FstScopeType,
    },
    UpScope,
    /// A new variable. `alias_of` picks an earlier one to alias, and an alias always describes the
    /// same signal as its target, so it takes over the target's type.
    Var {
        tpe: VarType,
        name: String,
        alias_of: Option<u8>,
    },
}

#[derive(Debug, Arbitrary)]
enum Op {
    /// Advance the time by `delta` ticks. A delta of zero repeats the current time stamp.
    Time {
        delta: u16,
    },
    Value {
        var: u8,
        bits: u64,
        four_state: bool,
    },
    Flush,
}

#[derive(Debug, Arbitrary)]
struct Waveform {
    hierarchy: Vec<Item>,
    ops: Vec<Op>,
}

/// Everything we compare between the two files, as parsed by `fst-reader`.
#[derive(Debug, PartialEq)]
struct Parsed {
    time_table: Vec<u64>,
    hierarchy: Vec<HierItem>,
    /// `(time, signal, value)`, sorted -- see `parse`
    changes: Vec<(u64, usize, String)>,
}

#[derive(Debug, PartialEq)]
enum HierItem {
    Scope(String),
    UpScope,
    Var {
        name: String,
        length: u32,
        handle: usize,
        is_alias: bool,
    },
}

fn parse(path: &Path) -> Result<Parsed, ReaderError> {
    let input = BufReader::new(std::fs::File::open(path).unwrap());
    let mut reader = FstReader::open_and_read_time_table(input)?;

    let time_table = reader.get_time_table().unwrap_or_default().to_vec();

    let mut hierarchy = vec![];
    reader.read_hierarchy(|entry| match entry {
        FstHierarchyEntry::Scope { name, .. } => hierarchy.push(HierItem::Scope(name)),
        FstHierarchyEntry::UpScope => hierarchy.push(HierItem::UpScope),
        FstHierarchyEntry::Var {
            name,
            length,
            handle,
            is_alias,
            ..
        } => hierarchy.push(HierItem::Var {
            name,
            length,
            handle: handle.get_index(),
            is_alias,
        }),
        _ => {}
    })?;

    let mut changes = vec![];
    reader
        .read_signals(&FstFilter::all(), |time, handle, value| {
            changes.push((
                time,
                handle.get_index(),
                match value {
                    FstSignalValue::String(value) => String::from_utf8_lossy(value).to_string(),
                    FstSignalValue::Real(value) => format!("{value:?}"),
                },
            ));
            Ok::<(), ()>(())
        })
        .map_err(|e| match e {
            ReadSignalsError::ReadError(e) => e,
            ReadSignalsError::CallbackError(()) => unreachable!("the callback never fails"),
        })?;
    // Value changes are reported section by section, and the writers do not agree on where the
    // section boundaries are, so only the set of changes is comparable, not their order.
    changes.sort();

    Ok(Parsed {
        time_table,
        hierarchy,
        changes,
    })
}

/// The value written for one signal, in the layout both writers expect: four state characters for
/// bit vectors, raw little endian bytes for reals.
fn value_for(tpe: VarType, bits: u64, four_state: bool) -> Vec<u8> {
    const ALPHABET: [u8; 4] = *b"01xz";
    match tpe {
        VarType::Real => f64::from_bits(bits).to_le_bytes().to_vec(),
        VarType::BitVec(width) => (0..(width as u32 + 1))
            .map(|i| {
                if four_state {
                    // mixes in x and z, so the value is not "digital" and gets written verbatim
                    ALPHABET[((bits >> ((i * 2) % 64)) & 0b11) as usize]
                } else {
                    // only 0 and 1, which takes the bit packing path
                    b'0' + ((bits >> (i % 64)) & 1) as u8
                }
            })
            .collect(),
    }
}

fn sanitize(s: &str) -> &str {
    // Remove any null bytes from the string. This Makes fstapi crate panic.
    s.split('\0')
        .next()
        .expect("split returns at least the full string")
}

fuzz_target!(|waveform: Waveform| {
    let outdir = tempdir().unwrap();
    let reference_path = outdir.path().join("fstapi.fst");
    let our_path = outdir.path().join("fst-writer.fst");

    let mut reference = fstapi::Writer::create(&reference_path, true)
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
    let mut ours = open_fst(&our_path, &info).unwrap();

    // one entry per variable: its type and its handle in either writer
    let mut vars: Vec<(VarType, fstapi::Handle, FstSignalId)> = vec![];
    let mut depth = 0usize;

    for item in waveform.hierarchy.iter() {
        match item {
            Item::Scope {
                name,
                component,
                tpe,
            } => {
                let name = sanitize(name);
                let component = sanitize(component);
                reference
                    .set_scope(*tpe as fstapi::ScopeType, name, component)
                    .unwrap();
                ours.scope(name, component, *tpe).unwrap();
                depth += 1;
            }
            Item::UpScope => {
                // popping the top level trips a debug assertion in `up_scope`
                if depth > 0 {
                    reference.set_upscope();
                    ours.up_scope().unwrap();
                    depth -= 1;
                }
            }
            Item::Var {
                name,
                tpe,
                alias_of,
            } => {
                let target = alias_of
                    .filter(|_| !vars.is_empty())
                    .map(|a| vars[a as usize % vars.len()]);
                let (tpe, reference_alias, our_alias) = match target {
                    Some((tpe, reference_handle, our_handle)) => {
                        (tpe, Some(reference_handle), Some(our_handle))
                    }
                    None => (*tpe, None, None),
                };

                let name = sanitize(name);

                let (reference_tpe, our_tpe, our_var_tpe, width) = match tpe {
                    VarType::Real => (
                        var_type::VCD_REAL,
                        FstSignalType::real(),
                        FstVarType::Real,
                        8,
                    ),
                    VarType::BitVec(width) => (
                        var_type::VCD_REG,
                        FstSignalType::bit_vec(width as u32 + 1),
                        FstVarType::Reg,
                        width as u32 + 1,
                    ),
                };

                let reference_handle = reference
                    .create_var(reference_tpe, var_dir::OUTPUT, width, name, reference_alias)
                    .unwrap();
                let our_handle = ours
                    .var(
                        name,
                        our_tpe,
                        our_var_tpe,
                        FstVarDirection::Output,
                        our_alias,
                    )
                    .unwrap();
                vars.push((tpe, reference_handle, our_handle));
            }
        }
    }

    if vars.is_empty() {
        return;
    }

    // leave the hierarchy in a consistent state
    while depth > 0 {
        reference.set_upscope();
        ours.up_scope().unwrap();
        depth -= 1;
    }

    let mut ours = ours.finish().unwrap();

    let mut time = 0u64;
    let mut saw_time_change = false;
    // A `signal_change` after a flush, with no new time step in between, hits a `todo!()` in
    // `SignalBuffer::signal_change`, so values are held back until the time advances again. Before
    // the first time change a flush is always a no-op -- values written then only go into the
    // frame, never into a section -- so those values still go through.
    let mut flushed_without_time = false;
    // Whether a value change is waiting to be written out. Values written before the first time
    // change do not count: the reference puts those in the frame alone (`fstapi.c:2932-2935`), so
    // its pending buffer is still empty as far as a flush is concerned. A flush with nothing pending corrupts
    // the reference's own output: `fstWriterFlushContext` queues it (fstapi.c:1838), then
    // `fstWriterFlushContextPrivate` returns early on `vchg_siz <= 1` (fstapi.c:1259) so no new
    // section starts, but `fstWriterEmitTimeChange` still appends `curtime` to the *current* time
    // chain (fstapi.c:3136-3140), where it is read back as a delta. For `10, 20, 30, flush, 40` the
    // reference then reports the time table as `[10, 20, 30, 60, 70]` with the value at 60, while
    // its own header still says the file ends at 40. There is nothing to compare against there, so
    // the flush is skipped instead.
    let mut pending_values = false;

    for op in &waveform.ops {
        match *op {
            Op::Time { delta } => {
                time += delta as u64;
                saw_time_change = true;
                if delta > 0 {
                    flushed_without_time = false;
                }
                reference.emit_time_change(time).unwrap();
                ours.time_change(time).unwrap();
            }
            Op::Value {
                var,
                bits,
                four_state,
            } => {
                if flushed_without_time {
                    continue;
                }
                let (tpe, reference_handle, our_handle) = vars[var as usize % vars.len()];
                let value = value_for(tpe, bits, four_state);
                reference
                    .emit_value_change(reference_handle, &value)
                    .unwrap();
                ours.signal_change(our_handle, &value).unwrap();
                pending_values |= saw_time_change;
            }
            Op::Flush => {
                if !pending_values {
                    continue;
                }
                reference.flush();
                ours.flush().unwrap();
                pending_values = false;
                flushed_without_time = saw_time_change;
            }
        }
    }

    ours.finish().unwrap();
    drop(reference);

    // Ignore cases where it fail even in the reference writer
    let Ok(reference) = parse(&reference_path) else {
        return;
    };
    let ours = parse(&our_path).unwrap();

    assert_eq!(reference, ours);
});
