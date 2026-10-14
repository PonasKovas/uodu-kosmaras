mod fmc_bus;

use embassy_stm32::gpio::Level;
use embassy_stm32::gpio::Output;
use embassy_stm32::gpio::Speed;
use embassy_stm32::peripherals::*;
use embassy_stm32::Peri;
use embassy_time::Timer;

use fmc_bus::FmcBus;

// ST7796 command registers
const CMD_SLPOUT: u8 = 0x11; // Sleep Out
const CMD_COLMOD: u8 = 0x3A; // Pixel format
const CMD_MADCTL: u8 = 0x36; // Memory access control (orientation / color order)
const CMD_CASET: u8 = 0x2A; // Column address window
const CMD_RASET: u8 = 0x2B; // Row address window
const CMD_INVON: u8 = 0x21; // Display inversion on (IPS panel)
const CMD_DISPON: u8 = 0x29; // Display on
const CMD_RAMWR: u8 = 0x2C; // Memory write
const CMD_TEON: u8 = 0x35; // Tearing Effect ON

pub const WIDTH: u16 = 480;
pub const HEIGHT: u16 = 320;

// DCX wired to FMC_A22; 16-bit accesses shift FMC addresses left by one, so
// the data window sits at address bit 23
const RS_ADDR_LINE: u32 = 22;
const CMD_OFFSET: usize = 0;
const DATA_OFFSET: usize = 1 << (RS_ADDR_LINE + 1);

pub struct St7796 {
    bus: FmcBus,
    _reset: Output<'static>,
}

impl St7796 {
    pub async fn init(
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
        nrst: Peri<'static, PD11>,
    ) -> Self {
        let mut bus = FmcBus::new(
            fmc, d0, d1, d2, d3, d4, d5, d6, d7, d8, d9, d10, d11, d12, d13, d14, d15, csx, dcx,
            wrx, rdx,
        );

        // Write timing for ST7796 @ 260 MHz FMC Clock (~73ns total cycle)
        bus.set_write_timing(3, 14); // 15.38 ns setup, 57.69 ns pulse

        // Read timing for ST7796 @ 260 MHz FMC Clock (~465ns total cycle)
        bus.set_read_timing(4, 115, 2); // 19.23 ns setup, 446.14 ns pulse, 7.69 ns turnaround

        // Hardware reset of the display controller
        let mut reset = Output::new(nrst, Level::High, Speed::Low);
        reset.set_low();
        Timer::after_millis(10).await;
        reset.set_high();
        Timer::after_millis(10).await;

        let mut panel = Self { bus, _reset: reset };

        // Sleep Out (wake up internal oscillators/charge pumps)
        panel.write_cmd(CMD_SLPOUT);
        Timer::after_millis(120).await; // MANDATORY 120ms wait after Sleep Out!

        // Set Pixel Format: 0x55 = 16 bits/pixel (RGB565)
        panel.write_cmd(CMD_COLMOD);
        panel.write_data(0x55);

        // Memory Access Control: orientation + RGB vs BGR order
        panel.write_cmd(CMD_MADCTL);
        panel.write_data(0x28); // Landscape (MV=1) + BGR=1

        // Enable Tearing Effect output (Mode 1: V-Blank only)
        panel.write_cmd(CMD_TEON);
        panel.write_data(0x00);

        // Default full-screen window
        panel.set_window(0, 0, WIDTH - 1, HEIGHT - 1);

        // Invert pixel values since this is an IPS screen
        panel.write_cmd(CMD_INVON);

        panel
    }

    /// Turn the panel on. Call after GRAM has been cleared.
    pub async fn display_on(&mut self) {
        self.write_cmd(CMD_DISPON);
        Timer::after_millis(20).await;
    }

    /// Set the active drawing window and enter RAMWR mode
    pub fn set_window(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) {
        self.write_cmd(CMD_CASET);
        self.write_data((x0 >> 8) as u16);
        self.write_data((x0 & 0xFF) as u16);
        self.write_data((x1 >> 8) as u16);
        self.write_data((x1 & 0xFF) as u16);

        self.write_cmd(CMD_RASET);
        self.write_data((y0 >> 8) as u16);
        self.write_data((y0 & 0xFF) as u16);
        self.write_data((y1 >> 8) as u16);
        self.write_data((y1 & 0xFF) as u16);

        self.write_cmd(CMD_RAMWR);
    }

    #[inline(always)]
    pub fn write_pixel(&mut self, color: u16) {
        self.write_data(color);
    }

    #[allow(dead_code)]
    pub fn write_pixels(&mut self, colors: &[u16]) {
        for &color in colors {
            self.write_data(color);
        }
    }

    #[inline(always)]
    fn write_cmd(&mut self, cmd: u8) {
        self.bus.write(CMD_OFFSET, cmd as u16);
    }

    /// Returns the absolute hardware address of the FMC data register
    pub fn data_ptr() -> *mut u16 {
        (fmc_bus::BANK1_BASE + DATA_OFFSET) as *mut u16
    }

    #[inline(always)]
    fn write_data(&mut self, data: u16) {
        self.bus.write(DATA_OFFSET, data);
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn read_data(&mut self) -> u16 {
        self.bus.read(DATA_OFFSET)
    }
}
