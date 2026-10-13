use embassy_executor::task;
use embassy_stm32::bind_interrupts;
use embassy_stm32::exti::{ExtiInput, InterruptHandler};
use embassy_stm32::gpio::Pull;
use embassy_stm32::interrupt::typelevel::EXTI15_10 as IrqExti15_10;
use embassy_stm32::peripherals::{EXTI12, PD12};
use embassy_stm32::Peri;

use crate::hardware::mdma::MdmaCh0;
use crate::hardware::st7796::St7796;

bind_interrupts!(pub struct TeIrqs {
    EXTI15_10 => InterruptHandler<IrqExti15_10>;
});

/// A double-buffer containing two 320-pixel 16-bit lines (1280 bytes total).
/// Must be exactly padded and aligned to 2048 bytes for the Cortex-M7 MPU region
/// so that surrounding variables do not accidentally become un-cached.
#[repr(C, align(2048))]
pub struct LineBuffers {
    pub buf_a: [u16; 320],
    pub buf_b: [u16; 320],
    pub _pad: [u16; 1024 - 640], // Pad remainder up to 2048 bytes
}

pub static mut DMA_BUFFERS: LineBuffers = LineBuffers {
    buf_a: [0; 320],
    buf_b: [0; 320],
    _pad: [0; 384],
};

/// Generates a smooth 256-color RGB565 rainbow palette using pure integer math
fn generate_palette() -> [u16; 256] {
    let mut palette = [0u16; 256];
    for i in 0..256 {
        let r = match i {
            0..=85 => 31 - (i * 31 / 85),
            171..=255 => (i - 171) * 31 / 84,
            _ => 0,
        } as u16;

        let g = match i {
            0..=85 => i * 63 / 85,
            86..=170 => 63 - ((i - 86) * 63 / 84),
            _ => 0,
        } as u16;

        let b = match i {
            86..=170 => (i - 86) * 31 / 84,
            171..=255 => 31 - ((i - 171) * 31 / 84),
            _ => 0,
        } as u16;

        // Pack bits into RGB565 format (5-bit Red, 6-bit Green, 5-bit Blue)
        palette[i] = (r << 11) | (g << 5) | b;
    }
    palette
}

/// Generates a fast 256-entry sine wave LUT (values 0..63) using integer math
fn generate_sin_lut() -> [u8; 256] {
    let mut lut = [0u8; 256];
    for i in 0..256 {
        let phase = i & 127;
        let val = if phase < 64 { phase } else { 128 - phase };
        let curve = (val * val) / 64;
        lut[i] = if i < 128 {
            (32 + curve / 2) as u8
        } else {
            (32 - curve / 2) as u8
        };
    }
    lut
}

#[inline(always)]
fn compute_line(
    buf: &mut [u16; 320],
    y: u16,
    t1: u8,
    t2: u8,
    t3: u8,
    sin_lut: &[u8; 256],
    palette: &[u16; 256],
) {
    let y_wave = sin_lut[((y >> 1) as u8).wrapping_add(t2) as usize] as usize;
    for x in 0..320u16 {
        let x_wave = sin_lut[(x as u8).wrapping_add(t1) as usize] as usize;
        let xy_wave = sin_lut[((x.wrapping_add(y) >> 1) as u8).wrapping_add(t3) as usize] as usize;
        let palette_idx = (x_wave + y_wave + xy_wave) & 0xFF;
        buf[x as usize] = palette[palette_idx];
    }
}

#[task]
pub async fn plasma_task(
    mut display: St7796,
    te_pin: Peri<'static, PD12>,
    exti: Peri<'static, EXTI12>,
    mut mdma: MdmaCh0,
) {
    let mut te_input = ExtiInput::new(te_pin, exti, Pull::None, TeIrqs);

    use defmt::error;
    use embassy_time::{with_timeout, Duration};

    // 1. Initialize LUTs once on startup
    let sin_lut = generate_sin_lut();
    let palette = generate_palette();

    // Animation phase accumulators
    let mut t1: u8 = 0;
    let mut t2: u8 = 0;
    let mut t3: u8 = 0;

    loop {
        // 2. Wait for display V-BLANK (Tearing Effect falling edge)
        match with_timeout(Duration::from_millis(100), te_input.wait_for_falling_edge()).await {
            Ok(_) => {}
            Err(_) => error!("TE Timeout! Is the TE pin pulsing?"),
        }

        // Reset display pointer to start of screen
        display.set_window(0, 0, 319, 479);

        let bufs = unsafe { core::ptr::addr_of_mut!(DMA_BUFFERS).as_mut().unwrap() };
        let dest_ptr = St7796::data_ptr();

        // Prime the very first line
        compute_line(&mut bufs.buf_a, 0, t1, t2, t3, &sin_lut, &palette);

        // 3. Render Plasma Frame Line-by-Line using Ping-Pong DMA
        for y in 0..480u16 {
            let current_buf = if y % 2 == 0 {
                bufs.buf_a.as_ptr()
            } else {
                bufs.buf_b.as_ptr()
            };

            // Launch DMA for the current line
            mdma.start_memory_to_peripheral_16bit(current_buf, dest_ptr, 320);

            // While DMA pushes the current line to the screen, CPU computes the NEXT line
            if y < 479 {
                let next_buf = if y % 2 == 0 {
                    &mut bufs.buf_b
                } else {
                    &mut bufs.buf_a
                };
                compute_line(next_buf, y + 1, t1, t2, t3, &sin_lut, &palette);
            }

            // Yield to executor and wait for this line's DMA to finish
            match with_timeout(Duration::from_millis(10), mdma.wait_complete()).await {
                Ok(_) => {}
                Err(_) => {
                    error!("MDMA Timeout on line {}!", y);
                    break; // Break the line loop to avoid spamming 480 errors
                }
            }
        }

        // 4. Advance plasma animation speeds
        t1 = t1.wrapping_add(2);
        t2 = t2.wrapping_add(3);
        t3 = t3.wrapping_add(1);
    }
}
