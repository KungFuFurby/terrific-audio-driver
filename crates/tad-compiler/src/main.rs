//! compiler binary

// SPDX-FileCopyrightText: © 2023 Marcus Rowe <undisbeliever@gmail.com>
//
// SPDX-License-Identifier: MIT

use clap::{Args, Parser, Subcommand};
use compiler::{compile_song, MappingsFile, SoundEffectsFile};

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

macro_rules! error {
    ($($arg:tt)*) => {{
        eprintln!($($arg)*);
        std::process::exit(1);
    }};
}

#[derive(Parser)]
#[command(author, version)]
#[command(about = "toname audio driver compiler")] // ::TODO rename this project or change this value::
#[command(arg_required_else_help = true)]
struct ArgParser {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compile common audio data
    Common(CompileCommonDataArgs),

    /// Compile MML song
    Song(CompileSongDataArgs),
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct OutputArg {
    #[arg(
        short = 'o',
        long = "output",
        value_name = "FILE",
        help = "output file"
    )]
    path: Option<PathBuf>,

    #[arg(long, help = "Write to stdout")]
    stdout: bool,
}

// Compile Common Audio Data
// =========================

#[derive(Args)]
struct CompileCommonDataArgs {
    #[command(flatten)]
    output: OutputArg,

    #[arg(value_name = "JSON_FILE", help = "instruments and mappings json file")]
    json_file: PathBuf,

    #[arg(value_name = "TXT_FILE", help = "sound_effects txt file")]
    sfx_file: PathBuf,
}

fn compile_common_data(args: CompileCommonDataArgs) {
    let mappings = load_mappings_file(args.json_file);
    let sfx_file = load_sfx_file(args.sfx_file);

    let data = match compiler::compile_common_audio_data(&mappings, &sfx_file) {
        Ok(data) => data,
        Err(errors) => {
            eprintln!("Cannot compile common audio data");
            for e in errors {
                eprintln!("{}", e);
            }
            std::process::exit(1);
        }
    };

    write_data(args.output, data);
}

//
// Compile Songs
// =============

#[derive(Args)]
struct CompileSongDataArgs {
    #[command(flatten)]
    output: OutputArg,

    #[arg(value_name = "JSON_FILE", help = "instruments and mappings json file")]
    json_file: PathBuf,

    #[arg(value_name = "MML_FILE", help = "mml song file")]
    mml_file: PathBuf,
}

fn compile_song_data(args: CompileSongDataArgs) {
    let file_name = file_name(&args.mml_file);

    let mml_text = load_mml_file(args.mml_file);

    let mappings = load_mappings_file(args.json_file);

    let data = match compile_song(&mml_text, &file_name, &mappings) {
        Ok(d) => d,
        Err(e) => error!("Cannot compile song\n{}", e),
    };

    write_data(args.output, data);
}

//
// Main
// ====

fn main() {
    let args = ArgParser::parse();

    match args.command {
        Command::Common(args) => compile_common_data(args),
        Command::Song(args) => compile_song_data(args),
    }
}

//
// File functions
// ==============

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .to_string()
}

fn load_mappings_file(path: PathBuf) -> MappingsFile {
    match compiler::load_mappings_file(path) {
        Ok(m) => m,
        Err(e) => error!("{}", e),
    }
}

fn load_sfx_file(path: PathBuf) -> SoundEffectsFile {
    let contents = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => error!("Cannot load sound effect file: {}", e),
    };

    compiler::sfx_file_from_string(contents, &path)
}

fn load_mml_file(path: PathBuf) -> String {
    match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => error!("Cannot load mml file: {}", e),
    }
}

fn write_data(out: OutputArg, data: Vec<u8>) {
    if let Some(path) = out.path {
        match fs::write(&path, data) {
            Ok(()) => (),
            Err(e) => error!("Error writing {}: {}", path.display(), e),
        }
    } else if out.stdout {
        match io::stdout().write_all(&data) {
            Ok(()) => (),
            Err(e) => error!("Error writing data: {}", e),
        }
    }
}
