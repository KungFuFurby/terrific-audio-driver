//! MML bytecode generator

// SPDX-FileCopyrightText: © 2023 Marcus Rowe <undisbeliever@gmail.com>
//
// SPDX-License-Identifier: MIT

use super::command_parser::{
    ManualVibrato, MmlCommand, MmlCommandWithPos, MpVibrato, PanCommand, PortamentoSpeed,
    VolumeCommand,
};
use super::identifier::Identifier;
use super::instruments::{EnvelopeOverride, MmlInstrument};
use super::tick_count_table::LineTickCounter;

use crate::bytecode::{
    BcTerminator, BcTicksKeyOff, BcTicksNoKeyOff, Bytecode, LoopCount, PitchOffsetPerTick,
    PlayNoteTicks, PortamentoVelocity, SubroutineId,
};
use crate::errors::{ErrorWithPos, MmlCommandError, ValueError};
use crate::file_pos::{FilePos, FilePosRange};
use crate::notes::{Note, SEMITONES_PER_OCTAVE};
use crate::pitch_table::PitchTable;
use crate::time::{TickClock, TickCounter};

use std::cmp::max;

pub const MAX_BROKEN_CHORD_NOTES: usize = 128;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LoopPoint {
    pub bytecode_offset: usize,
    pub tick_counter: TickCounter,
}

#[cfg(feature = "mml_tracking")]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BytecodePos {
    // Bytecode position (within the channel NOT the song)
    pub bc_pos: u16,
    // Character index within the input file
    pub char_index: u32,
}

#[derive(Debug, PartialEq)]
pub struct ChannelData {
    identifier: Identifier,

    bytecode: Vec<u8>,
    loop_point: Option<LoopPoint>,

    tick_counter: TickCounter,
    last_instrument: Option<usize>,

    // Some if this channel is a subroutine
    pub(super) bc_subroutine: Option<SubroutineId>,

    line_tick_counters: Vec<LineTickCounter>,
    pub(super) tempo_changes: Vec<(TickCounter, TickClock)>,

    #[cfg(feature = "mml_tracking")]
    pub(crate) bc_tracking: Vec<BytecodePos>,
}

impl ChannelData {
    pub fn identifier(&self) -> &Identifier {
        &self.identifier
    }
    pub fn bytecode(&self) -> &[u8] {
        &self.bytecode
    }
    pub fn loop_point(&self) -> Option<LoopPoint> {
        self.loop_point
    }
    pub fn tick_counter(&self) -> TickCounter {
        self.tick_counter
    }
    pub fn line_tick_counters(&self) -> &[LineTickCounter] {
        &self.line_tick_counters
    }
}

#[derive(Clone, PartialEq)]
enum MpState {
    Disabled,
    Manual,
    Mp(MpVibrato),
}

struct SkipLastLoopState {
    instrument: Option<usize>,
    prev_slurred_note: Option<Note>,
    vibrato: Option<ManualVibrato>,
}

struct MmlBytecodeGenerator<'a> {
    pitch_table: &'a PitchTable,
    instruments: &'a Vec<MmlInstrument>,
    subroutines: Option<&'a Vec<ChannelData>>,

    bc: Bytecode,

    line_tick_counters: Vec<LineTickCounter>,
    tempo_changes: Vec<(TickCounter, TickClock)>,

    instrument: Option<usize>,
    prev_slurred_note: Option<Note>,

    mp: MpState,
    vibrato: Option<ManualVibrato>,

    skip_last_loop_state: Option<SkipLastLoopState>,

    loop_point: Option<LoopPoint>,

    show_missing_set_instrument_error: bool,

    #[cfg(feature = "mml_tracking")]
    bc_tracking: Vec<BytecodePos>,
}

impl MmlBytecodeGenerator<'_> {
    fn new<'a>(
        pitch_table: &'a PitchTable,
        instruments: &'a Vec<MmlInstrument>,
        subroutines: Option<&'a Vec<ChannelData>>,
        is_subroutine: bool,
        n_commands: usize,
    ) -> MmlBytecodeGenerator<'a> {
        let _ = n_commands;

        MmlBytecodeGenerator {
            pitch_table,
            instruments,
            subroutines,
            bc: Bytecode::new(is_subroutine, false),
            line_tick_counters: Vec::new(),
            tempo_changes: Vec::new(),
            instrument: None,
            prev_slurred_note: None,
            mp: MpState::Disabled,
            vibrato: None,
            skip_last_loop_state: None,
            loop_point: None,
            show_missing_set_instrument_error: !is_subroutine,

            #[cfg(feature = "mml_tracking")]
            bc_tracking: Vec::with_capacity(n_commands),
        }
    }

    fn instrument_from_index(&self, i: usize) -> Result<&MmlInstrument, MmlCommandError> {
        match self.instruments.get(i) {
            Some(inst) => Ok(inst),
            None => Err(MmlCommandError::CannotFindInstrument),
        }
    }

    fn test_note(&mut self, note: Note) -> Result<(), MmlCommandError> {
        match self.instrument {
            Some(i) => {
                let inst = self.instrument_from_index(i)?;
                if note >= inst.first_note && note <= inst.last_note {
                    Ok(())
                } else {
                    Err(MmlCommandError::NoteOutOfRange(
                        note,
                        inst.first_note,
                        inst.last_note,
                    ))
                }
            }
            None => {
                if self.show_missing_set_instrument_error {
                    self.show_missing_set_instrument_error = false;
                    Err(MmlCommandError::CannotPlayNoteBeforeSettingInstrument)
                } else {
                    Ok(())
                }
            }
        }
    }

    fn calculate_vibrato_for_note(
        &self,
        mp: &MpVibrato,
        note: Note,
    ) -> Result<ManualVibrato, MmlCommandError> {
        if mp.depth_in_cents == 0 {
            return Err(MmlCommandError::MpDepthZero);
        }
        let inst = match self.instrument {
            Some(index) => self.instrument_from_index(index)?,
            None => return Err(MmlCommandError::CannotUseMpWithoutInstrument),
        };

        let pitch = self.pitch_table.pitch_for_note(inst.instrument_id, note);

        // Calculate the minimum and maximum pitches of the vibrato.
        // This produces more accurate results when cents is very large (ie, 400)
        let pow = f64::from(mp.depth_in_cents) / f64::from(SEMITONES_PER_OCTAVE as u32 * 100);
        let p1 = f64::from(pitch) * 2.0_f64.powf(-pow);
        let p2 = f64::from(pitch) * 2.0_f64.powf(pow);

        let qwt = u32::from(mp.quarter_wavelength_ticks.as_u8());

        let po_per_tick = f64::round((p2 - p1) / f64::from(qwt * 2));
        let po_per_tick = if po_per_tick > 1.0 { po_per_tick } else { 1.0 };

        if po_per_tick > u32::MAX.into() {
            return Err(MmlCommandError::MpPitchOffsetTooLarge(u32::MAX));
        }
        let po_per_tick = po_per_tick as u32;

        match po_per_tick.try_into() {
            Ok(po) => Ok(ManualVibrato {
                quarter_wavelength_ticks: mp.quarter_wavelength_ticks,
                pitch_offset_per_tick: po,
            }),
            Err(_) => Err(MmlCommandError::MpPitchOffsetTooLarge(po_per_tick)),
        }
    }

    fn split_play_note_length(
        length: TickCounter,
        is_slur: bool,
    ) -> Result<(PlayNoteTicks, TickCounter), ValueError> {
        let l = length.value();

        if !is_slur && l <= BcTicksKeyOff::MAX {
            return Ok((
                PlayNoteTicks::KeyOff(BcTicksKeyOff::try_from(l)?),
                TickCounter::new(0),
            ));
        }

        // The play_note instruction requires keyoff.
        let last_min = if is_slur {
            BcTicksNoKeyOff::MIN
        } else {
            BcTicksKeyOff::MIN
        };
        const MAX: u32 = BcTicksNoKeyOff::MAX;

        let pn = {
            if l <= MAX {
                l
            } else if l >= MAX + last_min {
                MAX
            } else {
                MAX - 1
            }
        };

        Ok((
            PlayNoteTicks::NoKeyOff(BcTicksNoKeyOff::try_from(pn)?),
            TickCounter::new(l - pn),
        ))
    }

    fn play_note_with_mp(
        &mut self,
        note: Note,
        length: TickCounter,
        is_slur: bool,
    ) -> Result<(), MmlCommandError> {
        let (pn_length, rest) = Self::split_play_note_length(length, is_slur)?;

        self.test_note(note)?;

        self.prev_slurred_note = if is_slur { Some(note) } else { None };

        match &self.mp {
            MpState::Manual => {
                self.bc.play_note(note, pn_length);
            }
            MpState::Disabled => {
                const POPT: PitchOffsetPerTick = PitchOffsetPerTick::new(0);

                let vibrato_disabled = match self.vibrato {
                    None => true,
                    Some(v) => v.pitch_offset_per_tick == POPT,
                };

                if vibrato_disabled {
                    self.bc.play_note(note, pn_length);
                } else {
                    self.bc
                        .set_vibrato_depth_and_play_note(POPT, note, pn_length);

                    if let Some(v) = &mut self.vibrato {
                        v.pitch_offset_per_tick = POPT;
                    }
                }
            }
            MpState::Mp(mp) => {
                let cv = self.calculate_vibrato_for_note(mp, note)?;

                if self.vibrato == Some(cv) {
                    self.bc.play_note(note, pn_length);
                } else {
                    match self.vibrato {
                        Some(sv) if sv.quarter_wavelength_ticks == cv.quarter_wavelength_ticks => {
                            self.bc.set_vibrato_depth_and_play_note(
                                cv.pitch_offset_per_tick,
                                note,
                                pn_length,
                            );
                        }
                        _ => {
                            self.bc
                                .set_vibrato(cv.pitch_offset_per_tick, cv.quarter_wavelength_ticks);
                            self.bc.play_note(note, pn_length);
                        }
                    }

                    self.vibrato = Some(cv);
                }
            }
        }

        self.rest_after_play_note(rest, is_slur)
    }

    fn rest_after_play_note(
        &mut self,
        length: TickCounter,
        is_slur: bool,
    ) -> Result<(), MmlCommandError> {
        if length.is_zero() {
            return Ok(());
        }
        if is_slur {
            return self.rest(length);
        }

        self.prev_slurred_note = None;

        let mut remaining_ticks = length.value();

        const MAX_REST: u32 = BcTicksNoKeyOff::MAX;
        const MAX_FINAL_REST: u32 = BcTicksKeyOff::MAX;
        const MIN_FINAL_REST: u32 = BcTicksKeyOff::MIN;
        const _: () = assert!(MIN_FINAL_REST > 1);

        while remaining_ticks > MAX_FINAL_REST {
            let l = if remaining_ticks >= MAX_REST + MIN_FINAL_REST {
                MAX_REST
            } else {
                MAX_REST - 1
            };
            self.bc.rest(BcTicksNoKeyOff::try_from(l).unwrap());
            remaining_ticks -= l;
        }

        self.bc
            .rest_keyoff(BcTicksKeyOff::try_from(remaining_ticks)?);

        Ok(())
    }

    fn rest(&mut self, length: TickCounter) -> Result<(), MmlCommandError> {
        let mut remaining_ticks = length.value();

        let rest_length = BcTicksNoKeyOff::try_from(BcTicksNoKeyOff::MAX).unwrap();
        const _: () = assert!(BcTicksNoKeyOff::MIN == 1);

        while remaining_ticks > rest_length.ticks() {
            self.bc.rest(rest_length);
            remaining_ticks -= rest_length.ticks();
        }

        self.bc.rest(BcTicksNoKeyOff::try_from(remaining_ticks)?);

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn portamento(
        &mut self,
        note1: Note,
        note2: Note,
        is_slur: bool,
        speed_override: Option<PortamentoSpeed>,
        total_length: TickCounter,
        delay_length: TickCounter,
        tie_length: TickCounter,
    ) -> Result<(), MmlCommandError> {
        assert!(delay_length < total_length);

        self.test_note(note1)?;
        self.test_note(note2)?;

        // Play note1 (if required)
        let note1_length = {
            if self.prev_slurred_note != Some(note1) {
                let note_1_length = max(TickCounter::new(1), delay_length);
                let (pn_length, rest) = Self::split_play_note_length(note_1_length, true)?;

                self.bc.play_note(note1, pn_length);
                self.rest_after_play_note(rest, true)?;
                note_1_length
            } else if !delay_length.is_zero() {
                self.rest(delay_length)?;
                delay_length
            } else {
                TickCounter::new(0)
            }
        };

        let portamento_length =
            TickCounter::new(total_length.value().wrapping_sub(note1_length.value()));
        if portamento_length.is_zero() {
            return Err(MmlCommandError::PortamentoDelayTooLong);
        }

        let velocity = match speed_override {
            Some(speed) => {
                if note1 < note2 {
                    i32::from(speed.as_u8())
                } else {
                    -i32::from(speed.as_u8())
                }
            }
            None => {
                let inst = match self.instrument {
                    Some(index) => self.instrument_from_index(index)?,
                    None => return Err(MmlCommandError::PortamentoRequiresInstrument),
                };
                let p1: i32 = self
                    .pitch_table
                    .pitch_for_note(inst.instrument_id, note1)
                    .into();
                let p2: i32 = self
                    .pitch_table
                    .pitch_for_note(inst.instrument_id, note2)
                    .into();

                let ticks = i32::try_from(portamento_length.value()).unwrap();

                (p2 - p1) / ticks
            }
        };
        let velocity = PortamentoVelocity::try_from(velocity)?;

        let (p_length, p_rest) =
            Self::split_play_note_length(tie_length + portamento_length, is_slur)?;
        self.bc.portamento(note2, velocity, p_length);

        self.prev_slurred_note = if is_slur { Some(note2) } else { None };

        self.rest_after_play_note(p_rest, is_slur)
    }

    fn broken_chord(
        &mut self,
        notes: &[Note],
        total_length: TickCounter,
        note_length: PlayNoteTicks,
    ) -> Result<(), MmlCommandError> {
        self.prev_slurred_note = None;

        if notes.is_empty() {
            return Err(MmlCommandError::NoNotesInBrokenChord);
        }
        if notes.len() > MAX_BROKEN_CHORD_NOTES {
            return Err(MmlCommandError::TooManyNotesInBrokenChord(notes.len()));
        }
        let n_notes: u32 = notes.len().try_into().unwrap();

        for n in notes {
            self.test_note(*n)?;
        }

        let expected_tick_counter = self.bc.get_tick_counter() + total_length;

        let total_ticks = total_length.value();

        // Number of ticks in the last note played outside the loop (if any).
        let mut last_note_ticks = total_ticks % note_length.ticks();

        // If tie is true, a keyoff is required after the loop.
        if note_length.is_slur() && last_note_ticks == 0 {
            last_note_ticks += note_length.ticks();
        }

        if last_note_ticks != 0 && last_note_ticks < BcTicksKeyOff::MIN {
            last_note_ticks = BcTicksKeyOff::MIN;
        }
        let last_note_ticks = last_note_ticks;

        if total_ticks < last_note_ticks {
            return Err(MmlCommandError::BrokenChordTotalLengthTooShort);
        }
        let notes_in_loop = (total_ticks - last_note_ticks) / note_length.ticks();

        let break_point = usize::try_from(notes_in_loop % n_notes).unwrap();
        let has_break_point: bool = break_point != 0;

        let n_loops = (notes_in_loop / n_notes) + u32::from(has_break_point);

        if n_loops < 2 {
            return Err(MmlCommandError::BrokenChordTotalLengthTooShort);
        }

        let n_loops = LoopCount::try_from(n_loops)?;

        self.bc.start_loop(Some(n_loops))?;

        for (i, n) in notes.iter().enumerate() {
            if i == break_point && i != 0 {
                self.bc.skip_last_loop()?;
            }
            self.bc.play_note(*n, note_length);
        }

        self.bc.end_loop(None)?;

        if last_note_ticks > 0 {
            // The last note to play is always a keyoff note.
            self.bc.play_note(
                notes[break_point],
                PlayNoteTicks::KeyOff(BcTicksKeyOff::try_from(last_note_ticks)?),
            )
        }

        if self.bc.get_tick_counter() != expected_tick_counter {
            return Err(MmlCommandError::BrokenChordTickCountMismatch(
                expected_tick_counter,
                self.bc.get_tick_counter(),
            ));
        }

        Ok(())
    }

    fn set_instrument(&mut self, inst_index: usize) -> Result<(), MmlCommandError> {
        if self.instrument == Some(inst_index) {
            return Ok(());
        }
        let inst = self.instrument_from_index(inst_index)?;
        let old_inst = match self.instrument {
            Some(i) => Some(self.instrument_from_index(i)?),
            None => None,
        };

        let i_id = inst.instrument_id;

        match old_inst {
            Some(old) if old.instrument_id == i_id => {
                // Instrument_id unchanged
                if inst.envelope_override != old.envelope_override {
                    match inst.envelope_override {
                        EnvelopeOverride::None => self.bc.set_instrument(i_id),
                        EnvelopeOverride::Adsr(adsr) => self.bc.set_adsr(adsr),
                        EnvelopeOverride::Gain(gain) => self.bc.set_gain(gain),
                    }
                }
            }
            _ => match inst.envelope_override {
                EnvelopeOverride::None => self.bc.set_instrument(i_id),
                EnvelopeOverride::Adsr(adsr) => self.bc.set_instrument_and_adsr(i_id, adsr),
                EnvelopeOverride::Gain(gain) => self.bc.set_instrument_and_gain(i_id, gain),
            },
        }

        self.instrument = Some(inst_index);

        Ok(())
    }

    fn call_subroutine(&mut self, s_id: SubroutineId) -> Result<(), MmlCommandError> {
        let sub: &ChannelData = match self.subroutines {
            Some(s) => match s.get(s_id.as_usize()) {
                Some(s) => s,
                None => return Err(MmlCommandError::CannotFindSubroutine),
            },
            None => return Err(MmlCommandError::CannotFindSubroutine),
        };

        // Calling a subroutine disables manual vibrato
        self.vibrato = None;
        if self.mp == MpState::Manual {
            self.mp = MpState::Disabled;
        }

        if let Some(inst) = sub.last_instrument {
            self.instrument = Some(inst);
        }

        self.bc.call_subroutine(s_id)?;

        Ok(())
    }

    fn set_manual_vibrato(&mut self, v: Option<ManualVibrato>) {
        self.mp = MpState::Manual;
        match v {
            Some(v) => {
                self.vibrato = Some(v);
                self.bc
                    .set_vibrato(v.pitch_offset_per_tick, v.quarter_wavelength_ticks);
            }
            None => {
                self.vibrato = None;
                self.bc.disable_vibrato();
            }
        }
    }

    fn set_song_tick_clock(&mut self, tick_clock: TickClock) -> Result<(), MmlCommandError> {
        let tc = (self.bc.get_tick_counter(), tick_clock);
        self.tempo_changes.push(tc);

        self.bc.set_song_tick_clock(tick_clock)?;
        Ok(())
    }

    fn process_command(
        &mut self,
        command: &MmlCommand,
        pos: &FilePosRange,
    ) -> Result<(), MmlCommandError> {
        #[cfg(feature = "mml_tracking")]
        self.bc_tracking.push(BytecodePos {
            bc_pos: self.bc.get_bytecode_len().try_into().unwrap_or(0xffff),
            char_index: pos.index_start,
        });

        match command {
            MmlCommand::NoCommand => (),

            MmlCommand::NewLine => {
                self.line_tick_counters.push(LineTickCounter {
                    line_number: pos.line_number,
                    ticks: self.bc.get_tick_counter(),
                    in_loop: self.bc.is_in_loop(),
                });
            }

            &MmlCommand::SetLoopPoint => {
                if self.loop_point.is_some() {
                    return Err(MmlCommandError::LoopPointAlreadySet);
                }
                self.loop_point = Some(LoopPoint {
                    bytecode_offset: self.bc.get_bytecode_len(),
                    tick_counter: self.bc.get_tick_counter(),
                })
            }

            &MmlCommand::SetInstrument(inst_index) => {
                self.set_instrument(inst_index)?;
            }

            &MmlCommand::CallSubroutine(s_id) => {
                self.call_subroutine(s_id)?;
            }

            &MmlCommand::SetManualVibrato(v) => {
                self.set_manual_vibrato(v);
            }

            &MmlCommand::SetMpVibrato(mp) => match mp {
                Some(mp) => self.mp = MpState::Mp(mp),
                None => self.mp = MpState::Disabled,
            },

            &MmlCommand::Rest(length) => {
                self.rest(length)?;
            }

            &MmlCommand::PlayNote {
                note,
                length,
                is_slur,
            } => {
                self.play_note_with_mp(note, length, is_slur)?;
            }

            &MmlCommand::PlayQuantizedNote {
                note,
                length: _,
                key_on_length,
                rest,
            } => {
                self.play_note_with_mp(note, key_on_length, false)?;
                self.rest(rest)?;
            }

            &MmlCommand::Portamento {
                note1,
                note2,
                is_slur,
                speed_override,
                total_length,
                delay_length,
                tie_length,
            } => {
                self.portamento(
                    note1,
                    note2,
                    is_slur,
                    speed_override,
                    total_length,
                    delay_length,
                    tie_length,
                )?;
            }

            MmlCommand::BrokenChord {
                notes,
                total_length,
                note_length,
            } => {
                self.broken_chord(notes, *total_length, *note_length)?;
            }

            MmlCommand::StartLoop => {
                self.bc.start_loop(None)?;
            }

            MmlCommand::SkipLastLoop => {
                self.bc.skip_last_loop()?;

                self.skip_last_loop_state = Some(SkipLastLoopState {
                    instrument: self.instrument,
                    prev_slurred_note: self.prev_slurred_note,
                    vibrato: self.vibrato,
                });
            }

            &MmlCommand::EndLoop(loop_count) => {
                self.bc.end_loop(Some(loop_count))?;

                if let Some(s) = &self.skip_last_loop_state {
                    self.instrument = s.instrument;
                    self.prev_slurred_note = s.prev_slurred_note;
                    self.vibrato = s.vibrato;
                }
                self.skip_last_loop_state = None;
            }

            &MmlCommand::ChangePanAndOrVolume(pan, volume) => match (pan, volume) {
                (Some(PanCommand::Absolute(p)), Some(VolumeCommand::Absolute(v))) => {
                    self.bc.set_pan_and_volume(p, v);
                }
                (pan, volume) => {
                    match volume {
                        Some(VolumeCommand::Absolute(v)) => self.bc.set_volume(v),
                        Some(VolumeCommand::Relative(v)) => self.bc.adjust_volume(v),
                        None => (),
                    }
                    match pan {
                        Some(PanCommand::Absolute(p)) => self.bc.set_pan(p),
                        Some(PanCommand::Relative(p)) => self.bc.adjust_pan(p),
                        None => (),
                    }
                }
            },

            &MmlCommand::SetEcho(e) => {
                if e {
                    self.bc.enable_echo();
                } else {
                    self.bc.disable_echo();
                }
            }

            &MmlCommand::SetSongTempo(bpm) => {
                self.set_song_tick_clock(bpm.to_tick_clock()?)?;
            }
            &MmlCommand::SetSongTickClock(tick_clock) => {
                self.set_song_tick_clock(tick_clock)?;
            }
        }

        Ok(())
    }
}

pub fn process_mml_commands(
    commands: &Vec<MmlCommandWithPos>,
    last_pos: FilePos,
    identifier: Identifier,
    subroutine_index: Option<u8>,
    pitch_table: &PitchTable,
    instruments: &Vec<MmlInstrument>,
    subroutines: Option<&Vec<ChannelData>>,
) -> Result<ChannelData, Vec<ErrorWithPos<MmlCommandError>>> {
    let mut errors = Vec::new();

    // Cannot have subroutine_index and subroutines list at the same time.
    if subroutine_index.is_some() {
        assert!(
            subroutines.is_none(),
            "Cannot set `subroutine_index` and `subroutines` vec at the same time"
        );
    }

    let mut gen = MmlBytecodeGenerator::new(
        pitch_table,
        instruments,
        subroutines,
        subroutine_index.is_some(),
        commands.len(),
    );

    for c in commands {
        match gen.process_command(c.command(), c.pos()) {
            Ok(()) => (),
            Err(e) => errors.push(ErrorWithPos(c.pos().clone(), e)),
        }
    }

    let tick_counter = gen.bc.get_tick_counter();
    let max_nested_loops = gen.bc.get_max_nested_loops();

    let terminator = match (subroutine_index, gen.loop_point) {
        (Some(_), Some(_)) => {
            panic!("Loop point not allowed in subroutine")
        }
        (None, None) => BcTerminator::DisableChannel,
        (Some(_), None) => BcTerminator::ReturnFromSubroutine,
        (None, Some(lp)) => {
            if lp.tick_counter == tick_counter {
                errors.push(ErrorWithPos(
                    last_pos.to_range(1),
                    MmlCommandError::NoTicksAfterLoopPoint,
                ));
            }
            BcTerminator::LoopChannel
        }
    };

    let bytecode = match gen.bc.bytecode(terminator) {
        Ok(b) => b,
        Err(e) => {
            errors.push(ErrorWithPos(
                last_pos.to_range(1),
                MmlCommandError::BytecodeError(e),
            ));
            Vec::new()
        }
    };

    let bc_subroutine =
        subroutine_index.map(|si| SubroutineId::new(si, tick_counter, max_nested_loops));

    if errors.is_empty() {
        Ok(ChannelData {
            identifier,
            bytecode,
            loop_point: gen.loop_point,
            tick_counter,
            last_instrument: gen.instrument,
            bc_subroutine,
            line_tick_counters: gen.line_tick_counters,
            tempo_changes: gen.tempo_changes,

            #[cfg(feature = "mml_tracking")]
            bc_tracking: gen.bc_tracking,
        })
    } else {
        Err(errors)
    }
}