// Copyright 2024 Cornell University
// released under BSD 3-Clause License
// author: Kevin Laeufer <laeufer@cornell.edu>
//
// Small utility that reads in a VCD, GHW or FST file with wellen and then
// writes out the FST with the fst-writer library.
// Similar to vcd2fst, just that the input format does not have to be specified
// by the command name.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "2fst")]
#[command(author = "Kevin Laeufer <laeufer@cornell.edu>")]
#[command(version)]
#[command(about = "Converts a VCD, GHW or FST file to an FST file.", long_about = None)]
struct Args {
    #[arg(value_name = "INPUT", index = 1)]
    input: std::path::PathBuf,
    #[arg(value_name = "FSTFILE", index = 2)]
    fst_file: std::path::PathBuf,
}

fn main() {
    let args = Args::parse();
}
