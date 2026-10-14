use embassy_executor::task;
use embassy_stm32::bind_interrupts;
use embassy_stm32::exti::{ExtiInput, InterruptHandler};
use embassy_stm32::gpio::Pull;
use embassy_stm32::interrupt::typelevel::EXTI15_10 as IrqExti15_10;
use embassy_stm32::peripherals::{EXTI12, PD12};
use embassy_stm32::Peri;

use crate::hardware::mdma::MdmaCh0;
use crate::hardware::st7796::{St7796, HEIGHT, WIDTH};

bind_interrupts!(pub struct TeIrqs {
    EXTI15_10 => InterruptHandler<IrqExti15_10>;
});

const W: usize = WIDTH as usize;
const HORIZON: u16 = HEIGHT / 2;

// DMA_BUFFERS lines are 480 pixels wide
const _: () = assert!(W <= 480);

/// A double-buffer containing two 480-pixel 16-bit lines (1920 bytes total).
/// Must be exactly padded and aligned to 2048 bytes for the Cortex-M7 MPU region
/// so that surrounding variables do not accidentally become un-cached.
#[repr(C, align(2048))]
pub struct LineBuffers {
    pub buf_a: [u16; 480],
    pub buf_b: [u16; 480],
    pub _pad: [u16; 1024 - 960], // Pad remainder up to 2048 bytes
}

pub static mut DMA_BUFFERS: LineBuffers = LineBuffers {
    buf_a: [0; 480],
    buf_b: [0; 480],
    _pad: [0; 64],
};

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
fn compute_line(buf: &mut [u16], y: u16, frame: u32, sin_lut: &[u8; 256]) {
    if y < HORIZON {
        // Sky and Sun
        let sky_r = (y as u32 * 31 / HORIZON as u32) as u16;
        let sky_b = 15 + (y as u32 * 16 / HORIZON as u32) as u16;
        let sky_color = (sky_r << 11) | sky_b;

        const SUN_CX: i32 = (W / 2) as i32;
        const SUN_CY: i32 = 100;
        const SUN_R2: i32 = 60 * 60;

        let dy_sun = (y as i32) - SUN_CY;
        let dy_sun_sq = dy_sun * dy_sun;

        let mut sun_g = 63 - ((y as i32 - 40) * 63 / 120);
        if sun_g < 0 {
            sun_g = 0;
        }
        if sun_g > 63 {
            sun_g = 63;
        }
        let sun_color = (31 << 11) | ((sun_g as u16) << 5);

        let mut cut_thickness = 0;
        if y > 80 {
            cut_thickness = (y - 80) * 3 / 16;
        }
        let phase = (y as u32).wrapping_add(16000).wrapping_sub(frame >> 1) % 16;
        let is_cut = phase < cut_thickness as u32;

        for x in 0..W {
            let idx1 = ((x as u32 / 2).wrapping_add(frame >> 2)) % 256;
            let idx2 = ((x as u32).wrapping_add(frame >> 1)) % 256;
            let mx1 = sin_lut[idx1 as usize] as i32;
            let mx2 = sin_lut[idx2 as usize] as i32;
            let mountain_y = HORIZON as i32 - 10 - (mx1 * 2 / 3) - (mx2 / 3);

            if (y as i32) > mountain_y {
                if (y as i32) == mountain_y + 1 || (y as i32) == mountain_y + 2 {
                    buf[x] = 0xF81F; // Magenta neon edge
                } else {
                    buf[x] = 0x0806; // Dark mountain fill
                }
            } else {
                let dx = (x as i32) - SUN_CX;
                let in_sun = (dx * dx + dy_sun_sq) < SUN_R2;

                if in_sun && !is_cut {
                    buf[x] = sun_color;
                } else {
                    buf[x] = sky_color;
                }
            }
        }
    } else {
        // Ground and Grid
        let dy = (y - HORIZON + 1) as u32; // 1 to HORIZON

        let z_scale = 8000 * 256;
        let time_offset = (frame % 128) * 300;

        let z_curr = (z_scale / dy) + time_offset;
        let z_next = (z_scale / (dy + 1)) + time_offset;

        let grid_size = 38400;
        let is_horiz = (z_curr / grid_size) != (z_next / grid_size);

        let bg_color = 0x1004; // (2 << 11) | 4

        let mut grid_r = 31;
        let mut grid_b = 31;
        if dy < 64 {
            grid_r = 2 + (dy * 29 / 64);
            grid_b = 4 + (dy * 27 / 64);
        }
        let line_grid_color = ((grid_r as u16) << 11) | (grid_b as u16);

        const HALF: usize = W / 2;

        // Left half: walk outward-to-center
        let mut v_mod = (HALF as i32 * 6) % (dy as i32);
        for x in 0..HALF {
            let is_vert = v_mod < 6;
            if is_horiz || is_vert {
                buf[x] = line_grid_color;
            } else {
                buf[x] = bg_color;
            }
            v_mod -= 6;
            while v_mod < 0 {
                v_mod += dy as i32;
            }
        }

        // Right half: walk center-to-outward
        let mut v_mod = 0;
        for x in HALF..W {
            let is_vert = v_mod < 6;
            if is_horiz || is_vert {
                buf[x] = line_grid_color;
            } else {
                buf[x] = bg_color;
            }
            v_mod += 6;
            while v_mod >= dy as i32 {
                v_mod -= dy as i32;
            }
        }
    }
}

#[task]
pub async fn plasma_task(
    mut display: St7796,
    te_pin: Peri<'static, PD12>,
    exti: Peri<'static, EXTI12>,
    mut mdma: MdmaCh0,
) {
    use defmt::error;
    use embassy_time::{with_timeout, Duration};

    let mut te = ExtiInput::new(te_pin, exti, Pull::None, TeIrqs);

    let sin_lut = generate_sin_lut();
    let mut frame: u32 = 0;

    // Clear GRAM to black while the panel is still off, so no power-on noise is shown
    {
        let bufs = unsafe { core::ptr::addr_of_mut!(DMA_BUFFERS).as_mut().unwrap() };
        bufs.buf_a.fill(0);
        display.set_window(0, 0, WIDTH - 1, HEIGHT - 1);
        for y in 0..HEIGHT {
            mdma.start_memory_to_peripheral_16bit(
                bufs.buf_a.as_ptr(),
                St7796::data_ptr(),
                W as u32,
            );
            if with_timeout(Duration::from_millis(10), mdma.wait_complete())
                .await
                .is_err()
            {
                error!("MDMA Timeout on clear line {}!", y);
                break;
            }
        }
    }
    display.display_on().await;

    loop {
        display.set_window(0, 0, WIDTH - 1, HEIGHT - 1);

        let bufs = unsafe { core::ptr::addr_of_mut!(DMA_BUFFERS).as_mut().unwrap() };
        let dest_ptr = St7796::data_ptr();
        let ptrs = [bufs.buf_a.as_mut_ptr(), bufs.buf_b.as_mut_ptr()];
        let line = |i: usize| unsafe { core::slice::from_raw_parts_mut(ptrs[i & 1], W) };

        // Prepare line 0 first so streaming starts the moment TE fires
        compute_line(line(0), 0, frame, &sin_lut);

        // TE mode 1 pulses at V-blank; start writing on the rising edge
        te.wait_for_rising_edge().await;

        for y in 0..HEIGHT {
            mdma.start_memory_to_peripheral_16bit(ptrs[y as usize & 1], dest_ptr, W as u32);

            if y + 1 < HEIGHT {
                compute_line(line(y as usize + 1), y + 1, frame, &sin_lut);
            }

            match with_timeout(Duration::from_millis(10), mdma.wait_complete()).await {
                Ok(_) => {}
                Err(_) => {
                    error!("MDMA Timeout on line {}!", y);
                    break;
                }
            }
        }

        frame = frame.wrapping_add(1);
    }
}
