use embassy_stm32::gpio::Level;
use embassy_stm32::gpio::Output;
use embassy_stm32::gpio::Speed;
use embassy_stm32::peripherals::*;
use embassy_stm32::Peri;
use embassy_time::Timer;

use crate::display::fmc::DisplayFMC;

mod fmc;

pub struct Display {
    fmc: DisplayFMC,
}

impl Display {
    pub async fn new(
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
        _te: Peri<'static, PD12>,
        _nrst: Peri<'static, PD11>,
    ) -> Self {
        // RESET the display controller chip
        ////////////////////////////////////
        let mut reset = Output::new(_nrst, Level::High, Speed::Low);
        reset.set_low();
        Timer::after_millis(10).await;
        reset.set_high();
        Timer::after_millis(10).await;

        let mut fmc = fmc::DisplayFMC::new(
            fmc, d0, d1, d2, d3, d4, d5, d6, d7, d8, d9, d10, d11, d12, d13, d14, d15, csx, dcx,
            wrx, rdx,
        );

        // 1. Sleep Out (Wake up internal oscillators/charge pumps)
        fmc.write_cmd(0x11);
        Timer::after_millis(120).await; // MANDATORY 120ms wait after Sleep Out!

        // Set Pixel Format (COLMOD = 0x3A)
        // 0x55 = 16 bits/pixel (RGB565)
        fmc.write_cmd(0x3A);
        fmc.write_data(0x55);

        // Memory Access Control (MADCTL = 0x36)
        // Controls orientation and RGB vs BGR order (0x48 or 0x00 is standard)
        fmc.write_cmd(0x36);
        fmc.write_data(0x48);

        // Set Column Address Window (CASET = 0x2A) -> 0 to 319 (for 320x480 screen)
        fmc.write_cmd(0x2A);
        fmc.write_data(0x00); // Start Column High
        fmc.write_data(0x00); // Start Column Low
        fmc.write_data(0x01); // End Column High (0x013F = 319)
        fmc.write_data(0x3F); // End Column Low

        // Set Page/Row Address Window (RASET = 0x2B) -> 0 to 479
        fmc.write_cmd(0x2B);
        fmc.write_data(0x00); // Start Row High
        fmc.write_data(0x00); // Start Row Low
        fmc.write_data(0x01); // End Row High (0x01DF = 479)
        fmc.write_data(0xDF); // End Row Low

        // Invert the pixel values since its an IPS screen.
        // INVOFF is 0x20, INVON is 0x21
        fmc.write_cmd(0x21);

        // Display ON (DISPON = 0x29)
        fmc.write_cmd(0x29);
        Timer::after_millis(20).await;

        // RAMWR - start memory write
        fmc.write_cmd(0x2C);

        Self { fmc }
    }

    #[inline(always)]
    pub fn write(&mut self, data: u16) {
        self.fmc.write_data(data);
    }
}
