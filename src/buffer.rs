// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>

use crate::io::{write_time_chain_update, write_value_change_section};
use crate::{FstSignalId, FstWriteError, Result};
use std::cmp::Ordering;
use std::io::{Seek, Write};

/// keeps track of signal values before writing them to disk
pub(crate) struct SignalBuffer {
    start_time: u64,
    end_time: u64,
    /// contains the delta encoded and compressed timetable
    time_table: Vec<u8>,
    time_table_entries: u64,
}

impl SignalBuffer {
    pub(crate) fn new(start_time: u64) -> Result<Self> {
        let mut time_table = Vec::with_capacity(16);
        write_time_chain_update(&mut time_table, 0, start_time)?;
        Ok(Self {
            start_time,
            end_time: start_time,
            time_table,
            time_table_entries: 1, // start time
        })
    }

    pub(crate) fn time_change(&mut self, new_time: u64) -> Result<()> {
        match new_time.cmp(&self.end_time) {
            Ordering::Less => {
                Err(FstWriteError::TimeDecrease(self.end_time, new_time))
            }
            Ordering::Equal => Ok(()),
            Ordering::Greater => {
                write_time_chain_update(
                    &mut self.time_table,
                    self.end_time,
                    new_time,
                )?;
                self.time_table_entries += 1;
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
        todo!()
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
            self.time_table_entries,
        )?;
        // TODO: recycle?
        Ok(())
    }
}
