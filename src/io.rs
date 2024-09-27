// Copyright 2023 The Regents of the University of California
// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::{
    FstFileType, FstScopeType, FstSignalId, FstSignalType, FstVarDirection,
    FstVarType, FstWriteError, Result,
};
use std::io::{Seek, SeekFrom, Write};

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

fn write_c_str(output: &mut impl Write, value: impl AsRef<str>) -> Result<()> {
    let bytes = value.as_ref().as_bytes();
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
#[derive(Debug, PartialEq)]
enum BlockType {
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

//////////////// Header
#[derive(Debug, PartialEq)]
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
    pub(crate) file_type: FstFileType,
    pub(crate) time_zero: u64,
}

pub(crate) fn write_header(
    output: &mut impl Write,
    header: &Header,
) -> Result<()> {
    write_u8(output, BlockType::Header as u8)?;
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

//////////////// Hierarchy

const HIERARCHY_TPE_VCD_SCOPE: u8 = 254;
const HIERARCHY_TPE_VCD_UP_SCOPE: u8 = 255;
const HIERARCHY_TPE_VCD_ATTRIBUTE_BEGIN: u8 = 252;
const HIERARCHY_TPE_VCD_ATTRIBUTE_END: u8 = 253;
const HIERARCHY_NAME_MAX_SIZE: usize = 512;
const HIERARCHY_ATTRIBUTE_MAX_SIZE: usize = 65536 + 4096;

pub(crate) fn write_hierarchy_bytes(
    output: &mut (impl Write + Seek),
    bytes: &[u8],
) -> Result<()> {
    write_u8(output, BlockType::HierarchyLZ4 as u8)?;
    // remember start to fix the section length afterward
    let start = output.stream_position()?;
    write_u64(output, 0)?; // dummy section length
    let uncompressed_length = bytes.len() as u64;
    write_u64(output, uncompressed_length)?;

    // we only support single LZ4 compression
    let out2 = {
        let compressed = lz4_flex::compress(bytes);
        output.write_all(&compressed)?;
        output
    };

    // fix section length
    let end = out2.stream_position()?;
    out2.seek(SeekFrom::Start(start))?;
    write_u64(out2, end - start)?;
    out2.seek(SeekFrom::Start(end))?;
    Ok(())
}

pub(crate) fn write_hierarchy_scope(
    output: &mut impl Write,
    name: impl AsRef<str>,
    component: impl AsRef<str>,
    tpe: FstScopeType,
) -> Result<()> {
    write_u8(output, HIERARCHY_TPE_VCD_SCOPE)?;
    write_u8(output, tpe as u8)?;
    debug_assert!(name.as_ref().len() <= HIERARCHY_NAME_MAX_SIZE);
    write_c_str(output, name)?;
    debug_assert!(component.as_ref().len() <= HIERARCHY_NAME_MAX_SIZE);
    write_c_str(output, component)?;
    Ok(())
}

pub(crate) fn write_hierarchy_up_scope(output: &mut impl Write) -> Result<()> {
    write_u8(output, HIERARCHY_TPE_VCD_UP_SCOPE)
}

pub(crate) fn write_hierarchy_var(
    output: &mut impl Write,
    tpe: FstVarType,
    direction: FstVarDirection,
    name: impl AsRef<str>,
    signal_tpe: FstSignalType,
    alias: Option<FstSignalId>,
) -> Result<()> {
    write_u8(output, tpe as u8)?;
    write_u8(output, direction as u8)?;
    debug_assert!(name.as_ref().len() <= HIERARCHY_NAME_MAX_SIZE);
    write_c_str(output, name)?;
    let length = signal_tpe.len();
    let raw_length = if tpe == FstVarType::Port {
        3 * length + 2
    } else {
        length
    };
    write_variant_u64(output, raw_length as u64)?;
    write_variant_u64(
        output,
        alias.map(|id| id.to_index()).unwrap_or_default() as u64,
    )?;
    Ok(())
}

//////////////// Geometry
pub(crate) fn write_geometry_start(
    output: &mut (impl Write + Seek),
) -> Result<u64> {
    write_u8(output, BlockType::Geometry as u8)?;
    // remember start to fix the section header
    let start = output.stream_position()?;
    write_u64(output, 0)?; // dummy section length
    write_u64(output, 0)?; // dummy uncompressed section length
    write_u64(output, 0)?; // dummy max handle
                           // return start to later fix up
    Ok(start)
}

pub(crate) fn write_geometry_finish(
    output: &mut (impl Write + Seek),
    start: u64,
    signal_count: u64,
) -> Result<()> {
    // remember the end
    let end = output.stream_position()?;
    // fix section header
    let section_len = end - start;
    output.seek(SeekFrom::Start(start))?;
    write_u64(output, section_len)?; // section length
    write_u64(output, section_len - 3 * 8)?; // uncompressed section _content_ length
    write_u64(output, signal_count)?; // max handle
                                      // return cursor back to end
    output.seek(SeekFrom::Start(end))?;
    Ok(())
}

pub(crate) fn write_geometry_entry(
    output: &mut impl Write,
    signal: FstSignalType,
) -> Result<()> {
    write_variant_u64(output, signal.to_file_format() as u64)?;
    Ok(())
}

//////////////// Value Change Data

#[inline]
pub(crate) fn write_time_chain_update(
    output: &mut impl Write,
    prev_time: u64,
    current_time: u64,
) -> Result<()> {
    debug_assert!(current_time >= prev_time);
    let delta = current_time - prev_time;
    write_variant_u64(output, delta)?;
    Ok(())
}

pub(crate) fn write_value_change_section(
    output: &mut (impl Write + Seek),
    start_time: u64,
    end_time: u64,
    time_table: &[u8],
    time_table_entries: u64,
) -> Result<()> {
    write_u8(output, BlockType::VcData as u8)?;
    // remember start to fix the section header
    let start = output.stream_position()?;
    write_u64(output, 0)?; // dummy section length
    write_u64(output, start_time)?;
    write_u64(output, end_time)?;

    // TODO: write actual data

    // time table at the end
    output.write_all(time_table)?;
    // we never compress the time table, so compressed and uncompressed length are always the same
    write_u64(output, time_table.len() as u64)?;
    write_u64(output, time_table.len() as u64)?;
    write_u64(output, time_table_entries)?;

    // fix section length
    let end = output.stream_position()?;
    let section_len = end - start;
    output.seek(SeekFrom::Start(start))?;
    write_u64(output, section_len)?;
    output.seek(SeekFrom::Start(end))?;
    Ok(())
}
