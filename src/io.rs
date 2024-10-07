// Copyright 2023 The Regents of the University of California
// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::FstWriteError::InvalidCharacter;
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
pub(crate) fn write_variant_i64(
    output: &mut impl Write,
    mut value: i64,
) -> Result<usize> {
    // often, the value is small
    if value <= 63 && value >= -64 {
        let byte = [value as u8 & 0x7f; 1];
        output.write_all(&byte)?;
        return Ok(1);
    }

    // calculate the number of bits we need to represent
    let bits = if value >= 0 {
        64 - value.leading_zeros() + 1
    } else {
        64 - value.leading_ones() + 1
    };
    let num_bytes = bits.div_ceil(7) as usize;

    let mut bytes = Vec::with_capacity(num_bytes);
    for ii in 0..num_bytes {
        let mark = if ii == num_bytes - 1 { 0 } else { 0x80 };
        bytes.push((value & 0x7f) as u8 | mark);
        value >>= 7;
    }
    output.write_all(&bytes)?;
    Ok(bytes.len())
}

#[inline]
pub(crate) fn write_u64(output: &mut impl Write, value: u64) -> Result<()> {
    let buf = value.to_be_bytes();
    output.write_all(&buf)?;
    Ok(())
}

#[inline]
pub(crate) fn write_u32(output: &mut impl Write, value: u32) -> Result<()> {
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
    pub(crate) max_signal_id: u64, // aka maxhandle
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
    write_u64(output, header.max_signal_id)?;
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

pub(crate) fn write_geometry(
    output: &mut (impl Write + Seek),
    signals: &[FstSignalType],
) -> Result<()> {
    write_u8(output, BlockType::Geometry as u8)?;
    // remember start to fix the section header
    let start = output.stream_position()?;
    write_u64(output, 0)?; // dummy section length
    write_u64(output, 0)?; // dummy uncompressed section length
    let max_handle = signals.len() as u64;
    write_u64(output, max_handle)?;

    for signal in signals.iter() {
        write_variant_u64(output, signal.to_file_format() as u64)?;
    }

    // remember the end
    let end = output.stream_position()?;
    // fix section header
    let section_len = end - start;
    output.seek(SeekFrom::Start(start))?;
    write_u64(output, section_len)?; // section length
    write_u64(output, section_len - 3 * 8)?; // uncompressed section _content_ length
                                             // return cursor back to end
    output.seek(SeekFrom::Start(end))?;

    Ok(())
}

//////////////// Value Change Data

#[inline]
pub(crate) fn write_one_bit_signal(
    output: &mut impl Write,
    time_delta: u64,
    value: u8,
) -> Result<()> {
    let vli = match value {
        b'0' | b'1' => {
            let bit = value - b'0';
            // 2-bits are used to encode the signal value
            let shift_count = 2;
            (time_delta << shift_count) | ((bit as u64) << 1)
        }
        _ => {
            if let Some(encoding) = encode_9_value(value) {
                // 4-bits are used to encode the signal value
                let shift_count = 4;
                (time_delta << shift_count) | ((encoding as u64) << 1) | 1
            } else {
                return Err(InvalidCharacter(value as char));
            }
        }
    };
    write_variant_u64(output, vli)?;
    Ok(())
}

#[inline]
pub(crate) fn write_multi_bit_signal(
    output: &mut impl Write,
    time_delta: u64,
    values: &[u8],
) -> Result<()> {
    let is_digital = is_digital(values);
    // write time delta
    write_variant_u64(output, (time_delta << 1) | (!is_digital as u64))?;
    // digital signals get a special encoding
    if is_digital {
        let mut wip_byte = 0u8;
        for (ii, value) in values.iter().enumerate() {
            let bit = *value - b'0';
            let bit_id = 7 - (ii & 0x7);
            wip_byte |= bit << bit_id;
            if bit_id == 0 {
                write_u8(output, wip_byte)?;
                wip_byte = 0;
            }
        }
        if values.len() % 8 != 0 {
            write_u8(output, wip_byte)?;
        }
    } else {
        output.write_all(values)?;
    }
    Ok(())
}

#[inline]
pub(crate) fn write_real_signal(
    output: &mut impl Write,
    time_delta: u64,
    value: f64,
) -> Result<()> {
    // write time delta, bit 0 should always be zero, otherwise we are triggering the "rare packed case"
    write_variant_u64(output, time_delta << 1)?;
    output.write_all(value.to_le_bytes().as_slice())?;
    Ok(())
}

#[inline]
fn is_digital(values: &[u8]) -> bool {
    values.iter().all(|v| matches!(*v, b'0' | b'1'))
}

#[inline]
fn encode_9_value(value: u8) -> Option<u8> {
    match value {
        b'x' | b'X' => Some(0),
        b'z' | b'Z' => Some(1),
        b'h' | b'H' => Some(2),
        b'u' | b'U' => Some(3),
        b'w' | b'W' => Some(4),
        b'l' | b'L' => Some(5),
        b'-' => Some(6),
        b'?' => Some(7),
        _ => None,
    }
}

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

/// start the value change section once the frame is complete
pub(crate) fn write_value_change_section_start(
    output: &mut (impl Write + Seek),
    frame_values: &[u8],
    max_signal_id: u32,
) -> Result<u64> {
    write_u8(output, BlockType::VcData as u8)?;
    // remember start to fix the section header
    let start = output.stream_position()?;
    write_u64(output, 0)?; // dummy section length
    write_u64(output, 0)?; // dummy start time
    write_u64(output, 0)?; // dummy end time

    // TODO: what is this value?
    write_u64(output, 0)?;

    // write frame
    let uncompressed_length = frame_values.len() as u64;
    // we do not support gzip right now and frames cannot be lz4 compressed
    let compressed_length = frame_values.len() as u64;
    write_variant_u64(output, uncompressed_length)?;
    write_variant_u64(output, compressed_length)?;
    write_variant_u64(output, max_signal_id as u64)?;
    output.write_all(frame_values)?;

    // return section start
    Ok(start)
}

const VALUE_CHANGE_PACK_TYPE_LZ4: u8 = b'4';

/// Writes out the offsets for each signal value stream in the "alias 2" encoding.
/// The original FST source code calls this data structure a "chain table"
fn write_offset_table(
    output: &mut (impl Write + Seek),
    offsets: &[u64],
) -> Result<()> {
    let mut zero_count = 0;
    for offset in offsets {
        if *offset == 0 {
            zero_count += 1;
        } else {
            // if there were any leading zeros, commit them to the output stream
            flush_zeros(output, &mut zero_count)?;
            // indicate that this is a real value and not just a sequence of zeros
            write_u8(output, 1)?;

            todo!("how does the actual encoding here work?");
        }
    }
    flush_zeros(output, &mut zero_count)?;

    Ok(())
}

#[inline]
fn flush_zeros(
    output: &mut (impl Write + Seek),
    zeros: &mut u32,
) -> Result<()> {
    if *zeros > 0 {
        // shifted by one because bit0 indicates whether we are dealing with a zero or a real offset
        let value = *zeros << 1;
        write_variant_u64(output, value as u64)?;
        *zeros = 0;
    }
    debug_assert_eq!(*zeros, 0);
    Ok(())
}

pub(crate) fn write_value_changes(
    output: &mut (impl Write + Seek),
    max_signal_id: u32,
) -> Result<()> {
    write_variant_u64(output, max_signal_id as u64)?;
    // we always use lz4 for compression
    write_u8(output, VALUE_CHANGE_PACK_TYPE_LZ4)?;

    // write "chain table", i.e. the data structure that tells us where each signal starts
    todo!()
}

pub(crate) fn write_value_change_section(
    output: &mut (impl Write + Seek),
    start_time: u64,
    end_time: u64,
    time_table: &[u8],
    time_table_entries: u64,
) -> Result<()> {
    write_u8(output, BlockType::VcDataDynamicAlias2 as u8)?;
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
