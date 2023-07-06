//! Audio Driver compiler

// SPDX-FileCopyrightText: © 2023 Marcus Rowe <undisbeliever@gmail.com>
//
// SPDX-License-Identifier: MIT

mod bytecode;
mod common_audio_data;
mod echo;
mod envelope;
mod mml_command_parser;
mod notes;
mod pitch_table;
mod samples;
mod songs;
mod sound_effects;
mod time;
mod value_newtypes;

pub mod bytecode_assembler;
pub mod data;
pub mod driver_constants;
pub mod errors;
pub mod mml;

pub use data::{
    load_project_file, validate_project_file_names, Name, ProjectFile, UniqueNamesProjectFile,
};
pub use envelope::{Adsr, Gain};
pub use notes::{Note, Octave, STARTING_OCTAVE};

pub use pitch_table::{build_pitch_table, PitchTable};
pub use samples::{build_sample_and_instrument_data, SampleAndInstrumentData};

pub use common_audio_data::build_common_audio_data;
pub use sound_effects::{compile_sound_effects_file, load_sound_effects_file, SoundEffectsFile};

pub use mml::parse_mml;
pub use songs::song_data;
