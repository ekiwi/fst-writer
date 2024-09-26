// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FstFileType {
    Verilog = 0,
    Vhdl = 1,
    VerilogVhdl = 2,
}

#[derive(Debug, Clone)]
pub struct FstInfo {
    pub start_time: u64,
    // TODO: better abstraction
    pub timescale_exponent: i8,
    pub version: String,
    pub date: String,
    pub file_type: FstFileType,
}

pub struct {
    
}