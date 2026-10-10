#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use panic_probe as _;

use uodu_kosmaras::hardware::mpu_conf::configure_mpu;
use uodu_kosmaras::hardware::rcc::rcc_config;
use uodu_kosmaras::hardware::st7796::St7796;
use uodu_kosmaras::{heartbeat, plasma, power, verify_hardware_uid};

const EXPECTED_M0_UID: [u8; 12] = [
    0x33, 0x0, 0x10, 0x0, 0x19, 0x51, 0x31, 0x32, 0x34, 0x35, 0x32, 0x30,
];

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    verify_hardware_uid(EXPECTED_M0_UID, "M0");
    info!("Booting M0");

    let mut config = embassy_stm32::Config::default();
    config.enable_debug_during_sleep = true;
    config.rcc = rcc_config();

    let p = embassy_stm32::init(config);
    let core_peri = cortex_m::Peripherals::take().unwrap();

    configure_mpu(core_peri.MPU, true);

    spawner
        .spawn(heartbeat::heartbeat_task(p.PE3.into()))
        .unwrap();

    spawner
        .spawn(power::power_task(p.PG1, p.PG0, p.EXTI0))
        .unwrap();

    let display = St7796::init(
        p.FMC, p.PD14, p.PD15, p.PD0, p.PD1, p.PE7, p.PE8, p.PE9, p.PE10, p.PE11, p.PE12, p.PE13,
        p.PE14, p.PE15, p.PD8, p.PD9, p.PD10, p.PD7, p.PE6, p.PD5, p.PD4, p.PD11,
    )
    .await;

    spawner.spawn(plasma::plasma_task(display)).unwrap();

    core::future::pending::<()>().await;
}
