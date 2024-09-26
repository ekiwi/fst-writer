// Copyright 2023 The Regents of the University of California
// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::{FstWriteError, Result};
use std::io::Write;

#[inline]
pub(crate) fn write_variant_u64(
    output: &mut impl Write,
    mut value: u64,
) -> Result<usize> {
    // often, the value is small
    if value <= 0x7f {
        let byte = [value as u8; 1];
        output.write_all(&byte)?;
        return Ok(1);
    }

    let mut bytes = Vec::with_capacity(10);
    while value != 0 {
        let next_value = value >> 7;
        let mask: u8 = if next_value == 0 { 0 } else { 0x80 };
        bytes.push((value & 0x7f) as u8 | mask);
        value = next_value;
    }
    assert!(bytes.len() <= 10);
    output.write_all(&bytes)?;
    Ok(bytes.len())
}

#[inline]
pub(crate) fn write_u64(output: &mut impl Write, value: u64) -> Result<()> {
    let buf = value.to_be_bytes();
    output.write_all(&buf)?;
    Ok(())
}

fn write_u8(output: &mut impl Write, value: u8) -> Result<()> {
    let buf = value.to_be_bytes();
    output.write_all(&buf)?;
    Ok(())
}

#[inline]
fn write_i8(output: &mut impl Write, value: i8) -> Result<()> {
    let buf = value.to_be_bytes();
    output.write_all(&buf)?;
    Ok(())
}

fn write_c_str(output: &mut impl Write, value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    output.write_all(bytes)?;
    write_u8(output, 0)?;
    Ok(())
}

#[inline]
fn write_c_str_fixed_length(
    output: &mut impl Write,
    value: &str,
    max_len: usize,
) -> Result<()> {
    let bytes = value.as_bytes();
    if bytes.len() >= max_len {
        return Err(FstWriteError::StringTooLong(max_len, value.to_string()));
    }
    output.write_all(bytes)?;
    let zeros = vec![0u8; max_len - bytes.len()];
    output.write_all(&zeros)?;
    Ok(())
}

#[inline]
fn write_f64(output: &mut impl Write, value: f64) -> Result<()> {
    // for f64, we have the option to use either LE or BE, we just need to be consistent
    let buf = value.to_le_bytes();
    output.write_all(&buf)?;
    Ok(())
}

const HEADER_LENGTH: u64 = 329;
const HEADER_VERSION_MAX_LEN: usize = 128;
const HEADER_DATE_MAX_LEN: usize = 119;
const DOUBLE_ENDIAN_TEST: f64 = std::f64::consts::E;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileType {
    Verilog = 0,
    Vhdl = 1,
    VerilogVhdl = 2,
}

#[repr(u8)]
#[derive(Debug, PartialEq)]
pub enum BlockType {
    Header = 0,
    VcData = 1,
    Blackout = 2,
    Geometry = 3,
    Hierarchy = 4,
    VcDataDynamicAlias = 5,
    HierarchyLZ4 = 6,
    HierarchyLZ4Duo = 7,
    VcDataDynamicAlias2 = 8,
    GZipWrapper = 254,
    Skip = 255,
}

#[derive(Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) struct Header {
    pub(crate) start_time: u64,
    pub(crate) end_time: u64,
    pub(crate) memory_used_by_writer: u64,
    pub(crate) scope_count: u64,
    pub(crate) var_count: u64,
    pub(crate) max_var_id_code: u64, // aka maxhandle
    pub(crate) vc_section_count: u64,
    pub(crate) timescale_exponent: i8,
    pub(crate) version: String,
    pub(crate) date: String,
    pub(crate) file_type: FileType,
    pub(crate) time_zero: u64,
}

#[allow(dead_code)]
pub(crate) fn write_header(
    output: &mut impl Write,
    header: &Header,
) -> Result<()> {
    write_u8(output, BlockType::Hierarchy as u8)?;
    write_u64(output, HEADER_LENGTH)?;
    write_u64(output, header.start_time)?;
    write_u64(output, header.end_time)?;
    write_f64(output, DOUBLE_ENDIAN_TEST)?;
    write_u64(output, header.memory_used_by_writer)?;
    write_u64(output, header.scope_count)?;
    write_u64(output, header.var_count)?;
    write_u64(output, header.max_var_id_code)?;
    write_u64(output, header.vc_section_count)?;
    write_i8(output, header.timescale_exponent)?;
    write_c_str_fixed_length(output, &header.version, HEADER_VERSION_MAX_LEN)?;
    write_c_str_fixed_length(output, &header.date, HEADER_DATE_MAX_LEN)?;
    write_u8(output, header.file_type as u8)?;
    write_u64(output, header.time_zero)?;
    Ok(())
}
