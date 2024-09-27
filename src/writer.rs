// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::io::{
    write_geometry_entry, write_geometry_finish, write_geometry_start,
    write_header, write_hierarchy_bytes, write_hierarchy_scope,
    write_hierarchy_up_scope, write_hierarchy_var, Header,
};
use crate::{
    FstInfo, FstScopeType, FstSignalId, FstSignalType, FstVarDirection,
    FstVarType, Result,
};

pub fn open_fst<P: AsRef<std::path::Path>>(
    path: P,
    info: &FstInfo,
) -> Result<FstHeaderWriter<std::io::BufWriter<std::fs::File>>> {
    FstHeaderWriter::open(path, info)
}

pub struct FstHeaderWriter<W: std::io::Write + std::io::Seek> {
    out: W,
    /// collect hierarchy section before compressing it
    hierarchy_buf: std::io::Cursor<Vec<u8>>,
    signal_count: u32,
    scope_depth: u64,
    geometry_start: u64,
}

impl FstHeaderWriter<std::io::BufWriter<std::fs::File>> {
    fn open<P: AsRef<std::path::Path>>(
        path: P,
        info: &FstInfo,
    ) -> Result<Self> {
        let f = std::fs::File::create(path)?;
        let mut out = std::io::BufWriter::new(f);
        write_header_meta_data(&mut out, info)?;
        let geometry_start = write_geometry_start(&mut out)?;
        Ok(Self {
            out,
            hierarchy_buf: std::io::Cursor::new(Vec::new()),
            signal_count: 0,
            scope_depth: 0,
            geometry_start,
        })
    }
}

impl<W: std::io::Write + std::io::Seek> FstHeaderWriter<W> {
    pub fn scope(
        &mut self,
        name: impl AsRef<str>,
        component: impl AsRef<str>,
        tpe: FstScopeType,
    ) -> Result<()> {
        self.scope_depth += 1;
        write_hierarchy_scope(&mut self.hierarchy_buf, name, component, tpe)
    }
    pub fn up_scope(&mut self) -> Result<()> {
        debug_assert!(self.scope_depth > 0, "no scope to pop");
        self.scope_depth -= 1;
        write_hierarchy_up_scope(&mut self.hierarchy_buf)
    }

    pub fn var(
        &mut self,
        name: impl AsRef<str>,
        signal_tpe: FstSignalType,
        tpe: FstVarType,
        dir: FstVarDirection,
        alias: Option<FstSignalId>,
    ) -> Result<FstSignalId> {
        write_hierarchy_var(
            &mut self.hierarchy_buf,
            tpe,
            dir,
            name,
            signal_tpe,
            alias,
        )?;
        if let Some(alias) = alias {
            debug_assert!(alias.to_index() <= self.signal_count);
            Ok(alias)
        } else {
            self.signal_count += 1;
            let id = FstSignalId::from_index(self.signal_count);
            write_geometry_entry(&mut self.out, signal_tpe)?;
            Ok(id)
        }
    }

    pub fn finish(mut self) -> Result<FstBodyWriter<W>> {
        debug_assert_eq!(
            self.scope_depth, 0,
            "missing calls to up-scope to close all scopes!"
        );
        write_geometry_finish(
            &mut self.out,
            self.geometry_start,
            self.signal_count as u64,
        )?;
        write_hierarchy_bytes(&mut self.out, &self.hierarchy_buf.into_inner())?;
        let next = FstBodyWriter { out: self.out };
        Ok(next)
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
