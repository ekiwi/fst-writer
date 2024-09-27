// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::io::{
    write_time_chain_update, write_u32, write_value_change_section,
    write_value_change_section_start, write_variant_u64,
};
use crate::{FstSignalId, FstSignalType, FstWriteError, Result};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::io::{Seek, Write};

/// keeps track of signal values before writing them to disk
pub(crate) struct SignalBuffer {
    start_time: u64,
    end_time: u64,
    signals: Vec<SignalInfo>,
    values: Vec<u8>,
    value_changes: Vec<u8>,
    /// contains the delta encoded and compressed timetable
    time_table: Vec<u8>,
    time_table_index: u32,
}

#[derive(Debug, Clone)]
struct SignalInfo {
    /// length in bytes / number of characters
    len: u32,
    /// starting offset in the value buffer
    offset: u32,
    /// time table index of the previous change
    prev_time_table_index: u32,
    /// pointer to the last value update
    back_pointer: u32,
}

fn gen_signal_info(signals: &[FstSignalType]) -> (Vec<SignalInfo>, usize) {
    let mut offset = 0;
    let mut out = Vec::with_capacity(signals.len());
    for signal in signals {
        out.push(SignalInfo {
            len: signal.len(),
            offset,
            prev_time_table_index: 0,
            back_pointer: 0,
        });
        offset += signal.len();
    }
    (out, offset as usize)
}

impl SignalBuffer {
    pub(crate) fn new(
        signals: &[FstSignalType],
        start_time: u64,
    ) -> Result<Self> {
        let mut time_table = Vec::with_capacity(16);
        write_time_chain_update(&mut time_table, 0, start_time)?;
        let (signals, values_len) = gen_signal_info(signals);
        let values = vec![b'x'; values_len];
        Ok(Self {
            start_time,
            end_time: start_time,
            signals,
            values,
            value_changes: vec![],
            time_table,
            time_table_index: 0,
        })
    }

    pub(crate) fn time_change(
        &mut self,
        output: &mut (impl Write + Seek),
        new_time: u64,
    ) -> Result<()> {
        match new_time.cmp(&self.end_time) {
            Ordering::Less => {
                Err(FstWriteError::TimeDecrease(self.end_time, new_time))
            }
            Ordering::Equal => Ok(()),
            Ordering::Greater => {
                let first_time_step = self.time_table.is_empty();

                write_time_chain_update(
                    &mut self.time_table,
                    self.end_time,
                    new_time,
                )?;
                if first_time_step {
                    write_value_change_section_start(
                        output,
                        &self.values,
                        (self.signals.len() + 1) as u32,
                    )?;
                } else {
                    self.time_table_index += 1;
                }
                self.end_time = new_time;
                Ok(())
            }
        }
    }

    pub(crate) fn signal_change(
        &mut self,
        signal_id: FstSignalId,
        value: &[u8],
    ) -> Result<()> {
        let info = match self.signals.get_mut(signal_id.to_array_index()) {
            Some(info) => info,
            None => return Err(FstWriteError::InvalidSignalId(signal_id)),
        };
        let len = info.len as usize;
        let start = info.offset as usize;
        let range = start..start + len;
        let value_cow = if value.len() == len {
            Cow::Borrowed(value)
        } else {
            let expanded = expand_special_vector_cases(value, len)
                .unwrap_or_else(|| {
                    panic!(
                        "Failed to parse four state value: {} for signal of size {}",
                        String::from_utf8_lossy(value),
                        len
                    )
                });
            assert_eq!(expanded.len(), len);
            Cow::Owned(expanded)
        };
        let value = value_cow.as_ref();
        debug_assert_eq!(value.len(), len);
        let first_time_step = self.time_table_index == 0;
        if first_time_step {
            self.values[range].copy_from_slice(value);
        } else {
            // check to see if there actually was a change
            if &self.values[range.clone()] == value {
                return Ok(());
            }
            self.values[range].copy_from_slice(value);
            // write down value change
            let start = self.value_changes.len();
            write_u32(&mut self.value_changes, info.back_pointer)?;
            write_variant_u64(
                &mut self.value_changes,
                (self.time_table_index - info.prev_time_table_index) as u64,
            )?;
            self.value_changes.extend_from_slice(value);
            // update info
            info.prev_time_table_index = self.time_table_index;
            info.back_pointer = start as u32;
        }
        Ok(())
    }

    pub(crate) fn finish(
        &mut self,
        output: &mut (impl Write + Seek),
    ) -> Result<()> {
        write_value_change_section(
            output,
            self.start_time,
            self.end_time,
            &mut self.time_table,
            self.time_table_index as u64 + 1, // zero based index
        )?;

        // TODO: recycle?
        Ok(())
    }
}

/// tries to expand common shortenings used in VCD encodings
#[inline]
fn expand_special_vector_cases(value: &[u8], len: usize) -> Option<Vec<u8>> {
    // if the value is actually longer than expected, there is nothing we can do
    if value.len() >= len {
        return None;
    }

    // zero, x or z extend
    match value[0] {
        b'1' | b'0' => {
            let mut extended = Vec::with_capacity(len);
            extended.resize(len - value.len(), b'0');
            extended.extend_from_slice(value);
            Some(extended)
        }
        b'x' | b'X' | b'z' | b'Z' => {
            let mut extended = Vec::with_capacity(len);
            extended.resize(len - value.len(), value[0]);
            extended.extend_from_slice(value);
            Some(extended)
        }
        _ => None, // failed
    }
}
