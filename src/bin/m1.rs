#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use panic_probe as _;

use uodu_kosmaras::current_regulator;
use uodu_kosmaras::hardware::mpu_conf::configure_mpu;
use uodu_kosmaras::hardware::rcc::rcc_config;
use uodu_kosmaras::{heartbeat, verify_hardware_uid};

const EXPECTED_M1_UID: [u8; 12] = [
    0x34, 0x0, 0x10, 0x0, 0x19, 0x51, 0x31, 0x32, 0x34, 0x35, 0x32, 0x30,
];

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    verify_hardware_uid(EXPECTED_M1_UID, "M1");
    info!("Booting M1");

    let mut config = embassy_stm32::Config::default();
    config.enable_debug_during_sleep = true;
    config.rcc = rcc_config();

    let p = embassy_stm32::init(config);
    let core_peri = cortex_m::Peripherals::take().unwrap();

    configure_mpu(core_peri.MPU, false);

    spawner
        .spawn(heartbeat::heartbeat_task(p.PE2.into()))
        .unwrap();

    spawner
        .spawn(current_regulator::regulator_task(
            p.PE7, p.PE8, p.DAC1, p.OPAMP2,
        ))
        .unwrap();

    core::future::pending::<()>().await;
}
