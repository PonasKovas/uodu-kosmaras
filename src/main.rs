#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use panic_probe as _;

use crate::mpu_conf::configure_mpu;

#[cfg(feature = "m1")]
mod current_regulator;

#[cfg(feature = "m0")]
mod display;

mod heartbeat;
mod mpu_conf;
mod power;
mod rcc;

// Sanity checks to ensure exact feature configuration
#[cfg(all(feature = "m0", feature = "m1"))]
compile_error!("Cannot compile for both M0 and M1 at the same time.");

#[cfg(not(any(feature = "m0", feature = "m1")))]
compile_error!("You must specify either the 'm0' or 'm1' feature.");

fn verify_hardware_uid() {
    let uid = embassy_stm32::uid::uid();

    info!("Device UID: {=[u8]:#02x}", uid);

    #[cfg(feature = "m0")]
    {
        const EXPECTED_M0_UID: [u8; 12] = [
            0x33, 0x0, 0x10, 0x0, 0x19, 0x51, 0x31, 0x32, 0x34, 0x35, 0x32, 0x30,
        ];

        if uid != EXPECTED_M0_UID {
            defmt::panic!("FATAL: M0 firmware flashed onto incorrect hardware! Aborting to prevent pin damage.");
        }
    }

    #[cfg(feature = "m1")]
    {
        const EXPECTED_M1_UID: [u8; 12] = [
            0x34, 0x0, 0x10, 0x0, 0x19, 0x51, 0x31, 0x32, 0x34, 0x35, 0x32, 0x30,
        ];
        if uid != EXPECTED_M1_UID {
            defmt::panic!("FATAL: M1 firmware flashed onto incorrect hardware! Aborting to prevent pin damage.");
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    verify_hardware_uid();

    #[cfg(feature = "m0")]
    info!("Booting M0");
    #[cfg(feature = "m1")]
    info!("Booting M1");

    let mut config = embassy_stm32::Config::default();
    config.enable_debug_during_sleep = true;
    config.rcc = rcc::rcc_config();

    let p = embassy_stm32::init(config);
    let core_peri = cortex_m::Peripherals::take().unwrap();

    configure_mpu(core_peri.MPU);

    #[cfg(feature = "m0")]
    spawner
        .spawn(heartbeat::heartbeat_task(p.PE3.into()))
        .unwrap();
    #[cfg(feature = "m1")]
    spawner
        .spawn(heartbeat::heartbeat_task(p.PE2.into()))
        .unwrap();

    #[cfg(feature = "m0")]
    spawner.spawn(power::power_task(p.PG1, p.PG0)).unwrap();

    #[cfg(feature = "m1")]
    spawner
        .spawn(current_regulator::regulator_task(
            p.PE7, p.PE8, p.DAC1, p.OPAMP2,
        ))
        .unwrap();

    #[cfg(feature = "m0")]
    {
        use embassy_executor::task;

        use crate::display::Display;

        let mut display = Display::new(
            p.FMC, p.PD14, p.PD15, p.PD0, p.PD1, p.PE7, p.PE8, p.PE9, p.PE10, p.PE11, p.PE12,
            p.PE13, p.PE14, p.PE15, p.PD8, p.PD9, p.PD10, p.PD7, p.PE6, p.PD5, p.PD4, p.PD12,
            p.PD11,
        )
        .await;

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
        pub async fn plasma_task(mut display: Display) {
            use embassy_time::Duration;

            // 1. Initialize LUTs once on startup
            let sin_lut = generate_sin_lut();
            let palette = generate_palette();

            // Animation phase accumulators
            let mut t1: u8 = 0;
            let mut t2: u8 = 0;
            let mut t3: u8 = 0;

            let target_frame_time = Duration::from_millis(16); // ~60 FPS

            loop {
                use embassy_time::Instant;

                let frame_start = Instant::now();

                // 4. Render Plasma Frame
                for y in 0..480u16 {
                    // Precalculate Y-wave once per row to save CPU cycles
                    let y_wave = sin_lut[((y >> 1) as u8).wrapping_add(t2) as usize] as usize;

                    for x in 0..320u16 {
                        let x_wave = sin_lut[(x as u8).wrapping_add(t1) as usize] as usize;
                        let xy_wave = sin_lut
                            [((x.wrapping_add(y) >> 1) as u8).wrapping_add(t3) as usize]
                            as usize;

                        // Combine 3 sine waves into a 0..255 index
                        let palette_idx = (x_wave + y_wave + xy_wave) & 0xFF;

                        // Push 16-bit RGB565 pixel straight to FMC
                        display.write(palette[palette_idx]);
                    }
                }

                // 5. Advance plasma animation speeds
                t1 = t1.wrapping_add(2);
                t2 = t2.wrapping_add(3);
                t3 = t3.wrapping_add(1);

                // 6. Frame Pacing (Ensures constant 60 FPS)
                let frame_duration = frame_start.elapsed();
                if frame_duration < target_frame_time {
                    use embassy_time::Timer;

                    Timer::after(target_frame_time - frame_duration).await;
                }
            }
        }

        // // ST7796 standard resolution is 320 x 480 = 153,600 pixels
        // for _ in 0..(320 * 480) {
        //     display.write(0x001F);
        // }
        spawner.spawn(plasma_task(display)).unwrap();
    }

    core::future::pending::<()>().await;
}
