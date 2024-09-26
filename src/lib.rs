// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

mod io;
mod writer;

type Result<T> = std::result::Result<T, FstWriteError>;

#[derive(Debug, thiserror::Error)]
pub enum FstWriteError {
    #[error("I/O operation failed")]
    Io(#[from] std::io::Error),
    #[error("The string is too large (max length: {0}): {1}")]
    StringTooLong(usize, String),
}

pub use writer::FstWriter;
