// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::io::{write_header, FstFileType, Header};
use crate::{FstWriteError, Result};

pub struct FstInfo {
    pub start_time: u64,
    // TODO: better abstraction
    pub timescale_exponent: i8,
    pub version: String,
    pub date: String,
    pub file_type: FstFileType,
}

pub fn open_fst<P: AsRef<std::path::Path>>(
    path: P,
    info: &FstInfo,
) -> Result<FstHeaderWriter<std::io::BufWriter<std::fs::File>>> {
    FstHeaderWriter::open(path, info)
}

pub struct FstHeaderWriter<W: std::io::Write + std::io::Seek> {
    out: W,
    scope_depth: u64,
}

impl FstHeaderWriter<std::io::BufWriter<std::fs::File>> {
    fn open<P: AsRef<std::path::Path>>(
        path: P,
        info: &FstInfo,
    ) -> Result<Self> {
        let f = std::fs::File::create(path)?;
        let mut out = std::io::BufWriter::new(f);
        write_header_meta_data(&mut out, info)?;
        Ok(Self {
            out,
            scope_depth: 0,
        })
    }
}

impl<W: std::io::Write + std::io::Seek> FstHeaderWriter<W> {
    pub fn scope(&mut self, name: impl AsRef<str>) {
        println!("scope {}", name.as_ref());
        self.scope_depth += 1;
    }
    pub fn up_scope(&mut self) {
        println!("up-scope");
        debug_assert!(self.scope_depth > 0, "no scope to pop");
        self.scope_depth -= 1;
    }

    pub fn var(&mut self, name: impl AsRef<str>, width: u32) {
        println!("var {} {width}", name.as_ref());
    }

    pub fn finish(self) -> FstBodyWriter<W> {
        debug_assert_eq!(
            self.scope_depth, 0,
            "missing calls to up-scope to close all scopes!"
        );
        FstBodyWriter { out: self.out }
    }
}

pub struct FstBodyWriter<W: std::io::Write + std::io::Seek> {
    out: W,
}

impl<W: std::io::Write + std::io::Seek> FstBodyWriter<W> {
    pub fn signal_change(&mut self) {
        todo!()
    }
}

const HEADER_POS: u64 = 0;

/// Writes the user supplied meta-data to the header. We will come back to the header later to
/// fill in other data.
fn write_header_meta_data<W: std::io::Write + std::io::Seek>(
    out: &mut W,
    info: &FstInfo,
) -> Result<()> {
    debug_assert_eq!(
        out.stream_position().unwrap(),
        HEADER_POS,
        "We expect the header to be written at position {HEADER_POS}"
    );
    let header = Header {
        start_time: info.start_time,
        end_time: info.start_time,
        memory_used_by_writer: 0,
        scope_count: 0,
        var_count: 0,
        max_var_id_code: 0,
        vc_section_count: 0,
        timescale_exponent: info.timescale_exponent,
        version: info.version.clone(),
        date: info.date.clone(),
        file_type: info.file_type,
        time_zero: 0,
    };
    write_header(out, &header)
}

pub struct FstHierarchyWriter {}
