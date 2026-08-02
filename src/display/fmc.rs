use core::mem;
use embassy_stm32::fmc::Fmc;
use embassy_stm32::gpio::AfType;
use embassy_stm32::gpio::Flex;
use embassy_stm32::gpio::Level;
use embassy_stm32::gpio::Output;
use embassy_stm32::gpio::OutputType;
use embassy_stm32::gpio::Speed;
use embassy_stm32::peripherals::*;
use embassy_stm32::Peri;
use embassy_time::Timer;
use stm32_metapac as pac;

const LCD_BASE: usize = 0x6000_0000; // FMC Bank 1/1
const RS_ADDR_LINE: u32 = 22; // DCX wired to FMC_A22
const LCD_CMD_ADDR: usize = LCD_BASE;
const LCD_DATA_ADDR: usize = LCD_BASE | (1usize << (RS_ADDR_LINE + 1));

pub struct DisplayFMC {}

impl DisplayFMC {
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
        csx: Peri<'static, PD7>,
        dcx: Peri<'static, PE6>,
        wrx: Peri<'static, PD5>,
        rdx: Peri<'static, PD4>,
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
        init_fmc_pin(d14);
        init_fmc_pin(d10);
        init_fmc_pin(d11);
        init_fmc_pin(d12);
        init_fmc_pin(d13);
        init_fmc_pin(d15);

        init_fmc_pin(csx);
        init_fmc_pin(dcx);
        init_fmc_pin(wrx);
        init_fmc_pin(rdx);

        let mut fmc = Fmc::new_raw(fmc);
        fmc.enable();
        fmc.memory_controller_enable();

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

        // Write Timing for ST7796 @ 260 MHz FMC Clock (~73ns total cycle)
        pac::FMC.bwtr(0).modify(|w| {
            w.set_accmod(pac::fmc::vals::Accmod::A);
            w.set_addset(3); // 15.38 ns setup
            w.set_addhld(0);
            w.set_datast(14); // 57.69 ns pulse
        });

        // Read Timing for ST7796 @ 260 MHz FMC Clock (~465ns total cycle)
        pac::FMC.btr(0).modify(|w| {
            w.set_accmod(pac::fmc::vals::Accmod::A);
            w.set_addset(4); // 19.23 ns setup
            w.set_addhld(0);
            w.set_datast(115); // 446.14 ns pulse
            w.set_busturn(2); // 7.69 ns bus turnaround
        });

        Self {}
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn write_cmd(&mut self, cmd: u8) {
        unsafe { core::ptr::write_volatile(LCD_CMD_ADDR as *mut u16, cmd as u16) };
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn write_data(&mut self, data: u16) {
        unsafe { core::ptr::write_volatile(LCD_DATA_ADDR as *mut u16, data) };
    }

    #[allow(unused)]
    pub fn write_data_slice(&mut self, data: &[u16]) {
        for &d in data {
            self.write_data(d);
        }
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn read_data(&mut self) -> u16 {
        unsafe { core::ptr::read_volatile(LCD_DATA_ADDR as *const u16) }
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
