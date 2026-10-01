// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::io::{
    write_multi_bit_signal, write_one_bit_signal, write_time_chain_update,
    write_value_change_section, write_variant_u64,
};
use crate::{FstSignalId, FstSignalType, FstWriteError, Result};
use std::borrow::Cow;
use std::io::{Seek, Write};

/// keeps track of signal values before writing them to disk
pub(crate) struct SignalBuffer {
    /// start time step of the current block
    ///
    /// Equal to previous block end_time or equal to the timestamp of the first value change in the
    /// initial block.
    start_time: u64,
    /// last time step written
    end_time: u64,
    /// constant signal meta-data
    signal_info: Vec<SignalInfo>,
    /// time table index of the previous change for each signal
    prev_time_table_index: Box<[u32]>,
    /// values for all signals in the first time step of this block
    frame: Box<[u8]>,
    /// copy of the frame with all value changes applied
    values: Box<[u8]>,
    /// contains the value changes
    value_changes: SingleVecLists,
    /// contains the delta encoded and compressed time table
    time_table: Vec<u8>,
    /// the index of the last time step written in [`Self::time_table`].
    time_table_index: u32,
    /// keep a vec allocation around for encoding signals
    write_buf: Vec<u8>,
    /// is this the first buffer for the file that we are writing?
    first_buffer: bool,
    /// was a value written before the first time change?
    signal_change_emitted: bool,
    /// start time of the first value change section
    first_time: u64,
    /// is a flush waiting to be acted on by the next time change?
    flush_pending: bool,
}

#[derive(Debug, Clone)]
struct SignalInfo {
    /// length in bytes / number of characters
    len: u32,
    /// starting offset in the value buffer
    offset: u32,
}

fn gen_signal_info(signals: &[FstSignalType]) -> (Vec<SignalInfo>, usize) {
    let mut offset = 0;
    let mut out = Vec::with_capacity(signals.len());
    for signal in signals {
        out.push(SignalInfo {
            len: signal.len(),
            offset,
        });
        offset += signal.len();
    }
    (out, offset as usize)
}

impl SignalBuffer {
    pub(crate) fn new(signals: &[FstSignalType]) -> Result<Self> {
        let (signal_info, values_len) = gen_signal_info(signals);
        let value_changes = SingleVecLists::new(signal_info.len());

        let mut values = vec![b'x'; values_len].into_boxed_slice();
        // Initialize reals as NaN.
        for (info, tpe) in signal_info.iter().zip(signals) {
            if tpe.is_real() {
                let range = info.offset as usize..(info.offset + info.len) as usize;
                values[range].copy_from_slice(&f64::NAN.to_le_bytes());
            }
        }

        let frame = values.clone();
        let prev_time_table_index = vec![0; signal_info.len()].into_boxed_slice();
        let time_table = Vec::with_capacity(16);
        Ok(Self {
            start_time: 0,
            end_time: 0,
            signal_info,
            prev_time_table_index,
            frame,
            values,
            value_changes,
            time_table,
            time_table_index: 0,
            write_buf: vec![],
            first_buffer: true,
            signal_change_emitted: false,
            first_time: 0,
            flush_pending: false,
        })
    }

    pub(crate) fn time_change(&mut self, new_time: u64) -> Result<()> {
        if self.is_initial_time() {
            // If any signal change was already emitted, start time at 0.
            self.start_time = if self.signal_change_emitted {
                0
            } else {
                new_time
            };
            self.first_time = self.start_time;
            self.start_time_step(new_time)?;
            return Ok(());
        }

        // In the first block we enter the conditional above, and subsequent blocks already have a
        // initial time step in the time table.
        debug_assert!(!self.time_table.is_empty());

        if new_time < self.end_time {
            return Err(FstWriteError::TimeDecrease(self.end_time, new_time));
        }

        self.time_table_index += 1;
        // write time table in compressed format
        write_time_chain_update(&mut self.time_table, self.end_time, new_time)?;
        self.end_time = new_time;
        Ok(())
    }

    /// Starts the first time step of a value change section: the values collected so far become
    /// the frame and the time is written relative to 0.
    fn start_time_step(&mut self, new_time: u64) -> Result<()> {
        debug_assert!(self.time_table.is_empty());
        debug_assert!(self.start_time <= new_time);
        self.frame = self.values.clone();
        write_time_chain_update(&mut self.time_table, 0, new_time)?;
        self.end_time = new_time;
        Ok(())
    }

    pub(crate) fn signal_change(&mut self, signal_id: FstSignalId, value: &[u8]) -> Result<()> {
        let info = match self.signal_info.get(signal_id.to_array_index()) {
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
        if self.is_initial_time() {
            self.values[range].copy_from_slice(value);
            self.signal_change_emitted = true;
        } else {
            // A section always holds at least one time step here: the initial time is handled
            // above, and [`Self::flush`] opens the next section with the time the previous one
            // closed at.
            debug_assert!(
                !self.time_table.is_empty(),
                "no time step to attach the value to"
            );
            // check to see if there actually was a change
            if &self.values[range.clone()] == value {
                return Ok(());
            }
            self.values[range].copy_from_slice(value);
            self.append_value_change(signal_id.to_array_index())?;
        }
        Ok(())
    }

    /// Writes down a value change for the current value of a signal in the current time step.
    fn append_value_change(&mut self, signal_idx: usize) -> Result<()> {
        let (offset, len) = {
            let info = &self.signal_info[signal_idx];
            (info.offset as usize, info.len as usize)
        };
        let time_table_idx_delta =
            (self.time_table_index - self.prev_time_table_index[signal_idx]) as u64;
        self.write_buf.clear();
        match &self.values[offset..offset + len] {
            [value] => write_one_bit_signal(&mut self.write_buf, time_table_idx_delta, *value)?,
            values => write_multi_bit_signal(&mut self.write_buf, time_table_idx_delta, values)?,
        }
        self.value_changes.append(signal_idx, &self.write_buf, None);

        // remember previous time-table index
        self.prev_time_table_index[signal_idx] = self.time_table_index;
        Ok(())
    }

    /// Returns true if the current section holds at least one value change.
    pub(crate) fn has_value_changes(&self) -> bool {
        !self.value_changes.is_empty()
    }

    /// Queues a flush for the next [`Self::time_change`] to act on. If less than 3 time steps has
    /// been issued for this block, the request is dropped.
    pub(crate) fn request_flush(&mut self) {
        // this limit is arbitrary, just matches fstapi
        if self.time_table_index <= 1 {
            return;
        }

        self.flush_pending = true;
    }

    /// Whether a queued flush is to be acted on now. The request is cleared either way.
    ///
    /// A request that finds nothing to write is dropped.
    pub(crate) fn take_pending_flush(&mut self) -> bool {
        std::mem::take(&mut self.flush_pending) && self.has_value_changes()
    }

    /// Returns true if the first [`time_change`] has yet to be issued.
    pub(crate) fn is_initial_time(&self) -> bool {
        self.time_table.is_empty() && self.first_buffer
    }

    pub(crate) fn end_time(&self) -> u64 {
        self.end_time
    }

    /// Mocks up a single time step at zero, containing the values of all signals.
    ///
    /// Used when the file is closed without the time ever advancing.
    pub(crate) fn mock_initial_time_step(&mut self) -> Result<()> {
        debug_assert!(self.is_initial_time(), "the time already advanced");
        self.time_change(0)?;
        self.clone_all_values()
    }

    /// Write down the current value of every signal as a value change in the current time step.
    ///
    /// Avoid losing signal changes made before the first time change. They are still stored in
    /// [`Self::frame`], but the reader, in certain cases, do not read it, and only considers the
    /// recorded value changes.
    fn clone_all_values(&mut self) -> Result<()> {
        for signal_idx in 0..self.signal_info.len() {
            self.append_value_change(signal_idx)?;
        }
        Ok(())
    }

    /// Start time of the first value change section.
    pub(crate) fn first_time(&self) -> u64 {
        self.first_time
    }

    fn num_time_table_entries(&self) -> u64 {
        if self.time_table.is_empty() {
            0
        } else {
            self.time_table_index as u64 + 1
        }
    }

    pub(crate) fn flush(&mut self, output: &mut (impl Write + Seek)) -> Result<u64> {
        // write data
        write_value_change_section(
            output,
            self.start_time,
            self.end_time,
            &self.frame,
            &self.time_table,
            self.num_time_table_entries(),
            |signal_idx: usize| self.value_changes.extract_list(signal_idx, None),
            self.signal_info.len(),
        )?;

        // reset data
        self.time_table_index = 0;
        for idx in self.prev_time_table_index.iter_mut() {
            *idx = 0;
        }
        self.start_time = self.end_time;
        self.time_table.clear();
        // The next section opens with the time this one closed at.
        write_time_chain_update(&mut self.time_table, 0, self.end_time)?;
        self.write_buf.clear();
        self.value_changes.clear();
        self.first_buffer = false;

        // TODO: recycle?
        Ok(self.end_time)
    }

    /// Returns the estimated size of all data structures that grow over time.
    pub(crate) fn size(&self) -> usize {
        self.time_table.len() + self.write_buf.len() + self.value_changes.size()
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
    fn append(&mut self, list_id: usize, data: &[u8], fixed_size: Option<usize>);
    fn extract_list(&self, list_id: usize, fixed_size: Option<usize>) -> Vec<u8>;
    fn clear(&mut self);
    fn size(&self) -> usize;
}

impl ValueLists for SingleVecLists {
    fn new(num_lists: usize) -> Self {
        let lists_last = vec![0u32; num_lists].into_boxed_slice();
        let data = vec![];
        Self { lists_last, data }
    }

    fn append(&mut self, list_id: usize, data: &[u8], fixed_size: Option<usize>) {
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

    fn extract_list(&self, list_id: usize, fixed_size: Option<usize>) -> Vec<u8> {
        let mut last = self.lists_last[list_id];
        // no list entries
        if last == 0 {
            vec![]
        } else {
            // find the first entry and calculate length
            let len = self.list_len(list_id, fixed_size);
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
                        out[remaining_len..remaining_len + len].copy_from_slice(src);
                    }
                }
                None => {
                    while last > 0 {
                        let start = last as usize - 1;
                        last = self.read_back_pointer(start);
                        let (len, len_skip) = read_variant_u64(self.data[start + 4..].as_ref());
                        let len = len as usize;
                        remaining_len -= len;
                        let start = start + 4 + len_skip; // skip back pointer and length
                        let src = &self.data[start..start + len];
                        out[remaining_len..remaining_len + len].copy_from_slice(src);
                    }
                }
            }
            debug_assert_eq!(remaining_len, 0);
            out
        }
    }

    fn clear(&mut self) {
        for e in self.lists_last.iter_mut() {
            *e = 0;
        }
        self.data.clear();
    }

    fn size(&self) -> usize {
        self.lists_last.len() * std::mem::size_of::<u32>() + self.data.len()
    }
}

impl SingleVecLists {
    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    #[inline]
    fn read_back_pointer(&self, start: usize) -> u32 {
        u32::from_le_bytes(self.data[start..start + 4].as_ref().try_into().unwrap())
    }

    /// Iterates from the back of the list to find the total size of all elements.
    fn list_len(&self, list_id: usize, fixed_size: Option<usize>) -> usize {
        let mut last = self.lists_last[list_id];
        if last == 0 {
            return 0;
        }
        let mut total_len = 0;
        match fixed_size {
            Some(len) => {
                while last > 0 {
                    let start = last as usize - 1;
                    last = self.read_back_pointer(start);
                    total_len += len;
                }
            }
            None => {
                while last > 0 {
                    let start = last as usize - 1;
                    last = self.read_back_pointer(start);
                    let (len, _) = read_variant_u64(self.data[start + 4..].as_ref());
                    total_len += len as usize;
                }
            }
        }

        total_len
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

    fn append(&mut self, list_id: usize, data: &[u8], _fixed_size: Option<usize>) {
        self.lists[list_id].extend_from_slice(data);
    }

    fn extract_list(&self, list_id: usize, _fixed_size: Option<usize>) -> Vec<u8> {
        self.lists[list_id].clone()
    }

    fn clear(&mut self) {
        for list in self.lists.iter_mut() {
            list.clear();
        }
    }

    fn size(&self) -> usize {
        self.lists.len() * std::mem::size_of::<Vec<u8>>()
            + self.lists.iter().map(|l| l.len()).sum::<usize>()
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
            dut.append(list_id, data, None);
            reference.append(list_id, data, None);
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
                    dut.append(list_id, entry, Some(len));
                    reference.append(list_id, entry, Some(len));
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
