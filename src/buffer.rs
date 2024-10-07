// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::io::{
    write_multi_bit_signal, write_one_bit_signal, write_time_chain_update,
    write_u32, write_value_change_section, write_value_change_section_start,
    write_variant_u64,
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
            let expanded = expand_special_vector_cases(value, len).unwrap_or_else(|| {
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
            let time_table_idx_delta =
                (self.time_table_index - info.prev_time_table_index) as u64;
            match value {
                [value] => write_one_bit_signal(
                    &mut self.value_changes,
                    time_table_idx_delta,
                    *value,
                )?,
                values => write_multi_bit_signal(
                    &mut self.value_changes,
                    time_table_idx_delta,
                    values,
                )?,
            }

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

/// Implements several append only lists inside a single `Vec` to store value changes.
struct SingleVecLists {
    /// offset in bytes of the last list entry
    lists_last: Box<[u32]>,
    data: Vec<u8>,
}

trait ValueLists {
    fn new(num_lists: usize) -> Self;
    fn append(
        &mut self,
        list_id: usize,
        data: &[u8],
        fixed_size: Option<usize>,
    );
    fn extract_list(
        &self,
        list_id: usize,
        fixed_size: Option<usize>,
    ) -> Vec<u8>;
}

impl ValueLists for SingleVecLists {
    fn new(num_lists: usize) -> Self {
        let lists_last = vec![0u32; num_lists].into_boxed_slice();
        let data = vec![];
        Self { lists_last, data }
    }

    fn append(
        &mut self,
        list_id: usize,
        data: &[u8],
        fixed_size: Option<usize>,
    ) {
        let back_pointer = self.lists_last[list_id];
        // new "last" entry, we add 1 to distinguish an empty list
        self.lists_last[list_id] = self.data.len() as u32 + 1;
        // remember the previous entry
        self.data.extend_from_slice(&back_pointer.to_le_bytes());
        // write the new data
        match fixed_size {
            Some(len) => {
                debug_assert_eq!(data.len(), len);
                self.data.extend_from_slice(data);
            }
            None => {
                // variable length
                write_variant_u64(&mut self.data, data.len() as u64).unwrap();
                self.data.extend_from_slice(data);
            }
        }
    }

    fn extract_list(
        &self,
        list_id: usize,
        fixed_size: Option<usize>,
    ) -> Vec<u8> {
        let mut last = self.lists_last[list_id];
        // no list entries
        if last == 0 {
            vec![]
        } else {
            // find the first entry and calculate length
            let (first, len) = self.list_start_and_len(list_id, fixed_size);
            let mut out = vec![0; len];
            let mut remaining_len = len;
            match fixed_size {
                Some(len) => {
                    while last > 0 {
                        let start = last as usize - 1;
                        last = self.read_back_pointer(start);
                        remaining_len -= len;
                        let start = start + 4; // skip back pointer
                        let src = &self.data[start..start + len];
                        out[remaining_len..remaining_len + len]
                            .copy_from_slice(src);
                    }
                }
                None => {
                    while last > 0 {
                        let start = last as usize - 1;
                        last = self.read_back_pointer(start);
                        let (len, len_skip) =
                            read_variant_u64(self.data[start + 4..].as_ref());
                        let len = len as usize;
                        remaining_len -= len;
                        let start = start + 4 + len_skip; // skip back pointer and length
                        let src = &self.data[start..start + len];
                        out[remaining_len..remaining_len + len]
                            .copy_from_slice(src);
                    }
                }
            }
            debug_assert_eq!(remaining_len, 0);
            out
        }
    }
}

impl SingleVecLists {
    #[inline]
    fn read_back_pointer(&self, start: usize) -> u32 {
        u32::from_le_bytes(
            self.data[start..start + 4].as_ref().try_into().unwrap(),
        )
    }

    /// Iterates from the back of the list to find the offset of the first element and
    /// the total size of all elements and
    fn list_start_and_len(
        &self,
        list_id: usize,
        fixed_size: Option<usize>,
    ) -> (usize, usize) {
        let mut last = self.lists_last[list_id];
        if last == 0 {
            return (0, 0);
        }
        let mut total_len = 0;
        let mut first_entry = 0;
        match fixed_size {
            Some(len) => {
                while last > 0 {
                    let start = last as usize - 1;
                    last = self.read_back_pointer(start);
                    total_len += len;
                    first_entry = start;
                }
            }
            None => {
                while last > 0 {
                    let start = last as usize - 1;
                    last = self.read_back_pointer(start);
                    first_entry = start;
                    let (len, _) =
                        read_variant_u64(self.data[start + 4..].as_ref());
                    total_len += len as usize;
                }
            }
        }

        (first_entry, total_len)
    }
}

/// Reference implementation in order to test `SingleVecLists`.
#[cfg(test)]
struct MultiVecLists {
    lists: Vec<Vec<u8>>,
}

#[cfg(test)]
impl ValueLists for MultiVecLists {
    fn new(num_lists: usize) -> Self {
        let lists = vec![vec![]; num_lists];
        Self { lists }
    }

    fn append(
        &mut self,
        list_id: usize,
        data: &[u8],
        _fixed_size: Option<usize>,
    ) {
        self.lists[list_id].extend_from_slice(data);
    }

    fn extract_list(
        &self,
        list_id: usize,
        _fixed_size: Option<usize>,
    ) -> Vec<u8> {
        self.lists[list_id].clone()
    }
}

#[inline]
pub(crate) fn read_variant_u64(input: &[u8]) -> (u64, usize) {
    let mut res = 0u64;
    for (ii, byte) in input.iter().take(10).enumerate() {
        // 64bit / 7bit = ~9.1
        let value = (*byte as u64) & 0x7f;
        res |= value << (7 * ii);
        if (*byte & 0x80) == 0 {
            return (res, ii + 1);
        }
    }
    unreachable!("should never get here!")
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn do_test_lists_var_len(data: &[(usize, Vec<u8>)]) {
        let num_lists = 16;
        let mut dut = SingleVecLists::new(num_lists);
        let mut reference = MultiVecLists::new(num_lists);

        // write data
        for (list_id, data) in data.iter() {
            let list_id = *list_id % num_lists;
            dut.append(list_id, &data, None);
            reference.append(list_id, &data, None);
        }

        // check results
        for list_id in 0..num_lists {
            assert_eq!(
                dut.extract_list(list_id, None),
                reference.extract_list(list_id, None)
            );
        }
    }

    fn do_test_lists_fixed_len(len: u8, list_data: &[Vec<u8>]) {
        let len = len as usize + 1;
        let num_lists = list_data.len();
        let mut dut = SingleVecLists::new(num_lists);
        let mut reference = MultiVecLists::new(num_lists);

        // write data
        for (list_id, data) in list_data.iter().enumerate() {
            for entry in data.as_slice().chunks(len) {
                if entry.len() == len {
                    dut.append(list_id, &entry, Some(len));
                    reference.append(list_id, &entry, Some(len));
                }
            }
        }

        // check results
        for list_id in 0..num_lists {
            assert_eq!(
                dut.extract_list(list_id, Some(len)),
                reference.extract_list(list_id, Some(len))
            );
        }
    }

    #[test]
    fn unit_test_fixed_len_lists() {
        let mut dut = SingleVecLists::new(2);
        dut.append(0, &[0], Some(1));
        assert_eq!(dut.extract_list(0, Some(1)), [0]);
    }

    proptest! {
        #[test]
        fn test_lists_var_len(data: Vec<(usize, Vec<u8>)>) {
            do_test_lists_var_len(&data);
        }
        #[test]
        fn test_lists_fixed_len(len: u8, data: Vec<Vec<u8>>) {
            do_test_lists_fixed_len(len, &data);
        }
    }
}
