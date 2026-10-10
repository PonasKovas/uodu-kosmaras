#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use panic_probe as _;

use crate::hardware::mpu_conf::configure_mpu;
use crate::hardware::rcc::rcc_config;

#[cfg(feature = "m1")]
mod current_regulator;

mod hardware;
#[cfg(feature = "m0")]
mod plasma;

mod heartbeat;
mod power;

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
    config.rcc = rcc_config();

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
        use crate::hardware::st7796::St7796;
        use crate::plasma::plasma_task;

        let display = St7796::init(
            p.FMC, p.PD14, p.PD15, p.PD0, p.PD1, p.PE7, p.PE8, p.PE9, p.PE10, p.PE11, p.PE12,
            p.PE13, p.PE14, p.PE15, p.PD8, p.PD9, p.PD10, p.PD7, p.PE6, p.PD5, p.PD4, p.PD11,
        )
        .await;

        spawner.spawn(plasma_task(display)).unwrap();
    }

    core::future::pending::<()>().await;
}
