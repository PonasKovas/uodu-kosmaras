#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_time::Timer;
use panic_probe as _;

mod heartbeat;
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
        // TODO: Replace with your actual M0 hex array once you read it from the console
        const EXPECTED_M0_UID: [u8; 12] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

        // Uncomment once you put the real serial number above!
        /*
        if *uid != EXPECTED_M0_UID {
            defmt::panic!("FATAL: M0 firmware flashed onto incorrect hardware! Aborting to prevent pin damage.");
        }
        */
    }

    #[cfg(feature = "m1")]
    {
        // TODO: Replace with your actual M1 hex array once you read it from the console
        const EXPECTED_M1_UID: [u8; 12] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

        // Uncomment once you put the real serial number above!
        /*
        if *uid != EXPECTED_M1_UID {
            defmt::panic!("FATAL: M1 firmware flashed onto incorrect hardware! Aborting to prevent pin damage.");
        }
        */
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

    #[cfg(feature = "m0")]
    spawner.spawn(heartbeat::heartbeat_task(p.PE3.into())).unwrap();
    #[cfg(feature = "m1")]
    spawner.spawn(heartbeat::heartbeat_task(p.PE2.into())).unwrap();

    #[cfg(feature = "m0")]
    spawner.spawn(power::power_task(p.PG1, p.PG0)).unwrap();

    info!("SUCCESS! CPU running rock-solid at 520 MHz!");

    loop {
        Timer::after_secs(1).await;
    }
}
