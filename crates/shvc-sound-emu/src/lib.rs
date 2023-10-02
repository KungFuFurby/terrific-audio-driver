//! SHVC-Sound emulator

extern crate cxx;
use cxx::UniquePtr;

#[cxx::bridge(namespace = "shvc_sound_emu")]
mod ffi {
    unsafe extern "C++" {
        include!("shvc-sound-emu.hpp");

        type ShvcSoundEmu;

        fn new_emulator() -> UniquePtr<ShvcSoundEmu>;

        fn power(self: Pin<&mut ShvcSoundEmu>, reset: bool);

        fn iplrom(self: &ShvcSoundEmu) -> &[u8; 64];
        fn iplrom_mut(self: Pin<&mut ShvcSoundEmu>) -> &mut [u8; 64];

        fn apuram(self: &ShvcSoundEmu) -> &[u8; 65536];
        fn apuram_mut(self: Pin<&mut ShvcSoundEmu>) -> &mut [u8; 65536];

        fn dsp_registers(self: &ShvcSoundEmu) -> &[u8; 128];

        fn set_echo_buffer_size(self: Pin<&mut ShvcSoundEmu>, esa: u8, edl: u8);

        fn set_spc_registers(
            self: Pin<&mut ShvcSoundEmu>,
            pc: u16,
            a: u8,
            x: u8,
            y: u8,
            psw: u8,
            sp: u8,
        );

        fn emulate(self: Pin<&mut ShvcSoundEmu>) -> &[i16; 512];
    }
}

pub struct ShvcSoundEmu {
    emu: UniquePtr<ffi::ShvcSoundEmu>,
}

impl ShvcSoundEmu {
    pub const AUDIO_BUFFER_SAMPLES: usize = 256;
    pub const AUDIO_BUFFER_SIZE: usize = Self::AUDIO_BUFFER_SAMPLES * 2;

    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let emu = ffi::new_emulator();
        if emu.is_null() {
            panic!("new_emulator() returned null");
        }
        Self { emu }
    }

    pub fn power(&mut self, reset: bool) {
        self.emu.pin_mut().power(reset)
    }

    pub fn iplrom(&self) -> &[u8; 64] {
        self.emu.iplrom()
    }
    pub fn iplrom_mut(&mut self) -> &mut [u8; 64] {
        self.emu.pin_mut().iplrom_mut()
    }

    pub fn apuram(&self) -> &[u8; 65536] {
        self.emu.apuram()
    }
    pub fn apuram_mut(&mut self) -> &mut [u8; 65536] {
        self.emu.pin_mut().apuram_mut()
    }

    pub fn dsp_registers(&self) -> &[u8; 128] {
        self.emu.dsp_registers()
    }

    pub fn set_echo_buffer_size(&mut self, esa: u8, edl: u8) {
        self.emu.pin_mut().set_echo_buffer_size(esa, edl)
    }

    pub fn set_spc_registers(&mut self, pc: u16, a: u8, x: u8, y: u8, psw: u8, sp: u8) {
        self.emu.pin_mut().set_spc_registers(pc, a, x, y, psw, sp)
    }

    pub fn emulate(&mut self) -> &[i16; Self::AUDIO_BUFFER_SIZE] {
        self.emu.pin_mut().emulate()
    }
}