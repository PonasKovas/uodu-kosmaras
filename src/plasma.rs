use embassy_executor::task;
use embassy_time::Duration;
use embassy_time::Instant;
use embassy_time::Timer;

use crate::hardware::st7796::St7796;

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

#[task]
pub async fn plasma_task(mut display: St7796) {
    // 1. Initialize LUTs once on startup
    let sin_lut = generate_sin_lut();
    let palette = generate_palette();

    // Animation phase accumulators
    let mut t1: u8 = 0;
    let mut t2: u8 = 0;
    let mut t3: u8 = 0;

    let target_frame_time = Duration::from_millis(16); // ~60 FPS

    loop {
        let frame_start = Instant::now();

        // 2. Render Plasma Frame
        for y in 0..480u16 {
            // Precalculate Y-wave once per row to save CPU cycles
            let y_wave = sin_lut[((y >> 1) as u8).wrapping_add(t2) as usize] as usize;

            for x in 0..320u16 {
                let x_wave = sin_lut[(x as u8).wrapping_add(t1) as usize] as usize;
                let xy_wave =
                    sin_lut[((x.wrapping_add(y) >> 1) as u8).wrapping_add(t3) as usize] as usize;

                // Combine 3 sine waves into a 0..255 index
                let palette_idx = (x_wave + y_wave + xy_wave) & 0xFF;

                // Push 16-bit RGB565 pixel straight to FMC
                display.write_pixel(palette[palette_idx]);
            }
        }

        // 3. Advance plasma animation speeds
        t1 = t1.wrapping_add(2);
        t2 = t2.wrapping_add(3);
        t3 = t3.wrapping_add(1);

        // 4. Frame Pacing (Ensures constant 60 FPS)
        let frame_duration = frame_start.elapsed();
        if frame_duration < target_frame_time {
            Timer::after(target_frame_time - frame_duration).await;
        }
    }
}
