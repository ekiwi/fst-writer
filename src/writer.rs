// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::{FstWriteError, Result};

pub struct FstWriter<W: std::io::Write + std::io::Seek> {
    out: W,
}

impl FstWriter<std::io::BufWriter<std::fs::File>> {
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let f = std::fs::File::create(path)?;
        let out = std::io::BufWriter::new(f);
        Ok(Self { out })
    }
}
