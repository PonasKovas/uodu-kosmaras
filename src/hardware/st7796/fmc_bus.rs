use core::mem;
use embassy_stm32::fmc::Fmc;
use embassy_stm32::gpio::AfType;
use embassy_stm32::gpio::Flex;
use embassy_stm32::gpio::OutputType;
use embassy_stm32::gpio::Speed;
use embassy_stm32::peripherals::*;
use embassy_stm32::Peri;
use stm32_metapac as pac;

const BANK1_BASE: usize = 0x6000_0000; // FMC Bank 1/1 (NE1)

pub struct FmcBus {}

impl FmcBus {
    pub fn new(
        fmc: Peri<'static, FMC>,
        d0: Peri<'static, PD14>,
        d1: Peri<'static, PD15>,
        d2: Peri<'static, PD0>,
        d3: Peri<'static, PD1>,
        d4: Peri<'static, PE7>,
        d5: Peri<'static, PE8>,
        d6: Peri<'static, PE9>,
        d7: Peri<'static, PE10>,
        d8: Peri<'static, PE11>,
        d9: Peri<'static, PE12>,
        d10: Peri<'static, PE13>,
        d11: Peri<'static, PE14>,
        d12: Peri<'static, PE15>,
        d13: Peri<'static, PD8>,
        d14: Peri<'static, PD9>,
        d15: Peri<'static, PD10>,
        ne1: Peri<'static, PD7>,
        a22: Peri<'static, PE6>,
        nwe: Peri<'static, PD5>,
        noe: Peri<'static, PD4>,
    ) -> Self {
        init_fmc_pin(d0);
        init_fmc_pin(d1);
        init_fmc_pin(d2);
        init_fmc_pin(d3);
        init_fmc_pin(d4);
        init_fmc_pin(d5);
        init_fmc_pin(d6);
        init_fmc_pin(d7);
        init_fmc_pin(d8);
        init_fmc_pin(d9);
        init_fmc_pin(d10);
        init_fmc_pin(d11);
        init_fmc_pin(d12);
        init_fmc_pin(d13);
        init_fmc_pin(d14);
        init_fmc_pin(d15);

        init_fmc_pin(ne1);
        init_fmc_pin(a22);
        init_fmc_pin(nwe);
        init_fmc_pin(noe);

        let mut fmc = Fmc::new_raw(fmc);
        fmc.enable();
        fmc.memory_controller_enable();

        // 16-bit async SRAM, no address/data mux, no burst, separate
        // read/write timing (extended mode)
        pac::FMC.bcr1().modify(|w| {
            w.set_mbken(true);
            w.set_muxen(false);
            w.set_mtyp(pac::fmc::vals::Mtyp::SRAM);
            w.set_mwid(pac::fmc::vals::Mwid::BITS16);
            w.set_faccen(false);
            w.set_bursten(false);
            w.set_wren(true);
            w.set_extmod(true);
            w.set_asyncwait(false);
            w.set_waiten(false);
            w.set_cburstrw(false);
        });

        Self {}
    }

    /// Write-cycle timing in HCLK cycles (async mode A): address setup +
    /// data pulse duration
    pub fn set_write_timing(&mut self, addset: u8, datast: u8) {
        pac::FMC.bwtr(0).modify(|w| {
            w.set_accmod(pac::fmc::vals::Accmod::A);
            w.set_addset(addset);
            w.set_addhld(0);
            w.set_datast(datast);
        });
    }

    /// Read-cycle timing in HCLK cycles (async mode A): address setup +
    /// data pulse duration + bus turnaround
    pub fn set_read_timing(&mut self, addset: u8, datast: u8, busturn: u8) {
        pac::FMC.btr(0).modify(|w| {
            w.set_accmod(pac::fmc::vals::Accmod::A);
            w.set_addset(addset);
            w.set_addhld(0);
            w.set_datast(datast);
            w.set_busturn(busturn);
        });
    }

    /// Write a 16-bit word at `offset` bytes into Bank 1
    #[inline(always)]
    pub fn write(&mut self, offset: usize, value: u16) {
        unsafe { core::ptr::write_volatile((BANK1_BASE + offset) as *mut u16, value) };
    }

    /// Read a 16-bit word from `offset` bytes into Bank 1
    #[allow(dead_code)]
    #[inline(always)]
    pub fn read(&mut self, offset: usize) -> u16 {
        unsafe { core::ptr::read_volatile((BANK1_BASE + offset) as *const u16) }
    }
}

/// Helper to configure any pin for FMC Alternate Function (AF12)
fn init_fmc_pin(pin: Peri<'static, impl embassy_stm32::gpio::Pin>) {
    let mut flex = Flex::new(pin);
    // Configures pin as Alternate Function (AF12 for FMC)
    flex.set_as_af_unchecked(
        12, // AF12 = FMC
        AfType::output(OutputType::PushPull, Speed::VeryHigh),
    );

    mem::forget(flex);
}
