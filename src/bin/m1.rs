#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use panic_probe as _;

use uodu_kosmaras::hardware::mpu_conf::configure_mpu;
use uodu_kosmaras::hardware::rcc::rcc_config;
use uodu_kosmaras::{current_regulator, peer_sync};
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
    let mut core_peri = cortex_m::Peripherals::take().unwrap();

    configure_mpu(core_peri.MPU, false, None);
    core_peri.SCB.enable_icache();

    spawner
        .spawn(heartbeat::heartbeat_task(p.PE2.into()))
        .unwrap();

    spawner
        .spawn(current_regulator::regulator_task(
            p.PE7, p.PE8, p.DAC1, p.OPAMP2,
        ))
        .unwrap();

    spawner
        .spawn(peer_sync::peer_sync_task(p.PG3.into(), p.PG2.into()))
        .unwrap();

    {
        use uodu_kosmaras::hardware::pssi::{ClockOut, Config as PssiConfig, Pssi16};
        use uodu_kosmaras::link_test;
        let pssi = Pssi16::new(
            p.PSSI,
            p.DMA2_CH0,
            None,
            p.PA6,
            p.PA4,
            p.PG9,
            p.PC6,
            p.PC7,
            p.PC8,
            p.PC9,
            p.PC11,
            p.PD3,
            p.PE5,
            p.PB9,
            p.PC10,
            p.PC12,
            p.PD6,
            p.PD2,
            p.PF11,
            p.PD13,
            p.PA5,
            p.PF10,
            PssiConfig::default(),
        );
        spawner.spawn(link_test::m1_tx_task(pssi)).unwrap();
    }

    core::future::pending::<()>().await;
}
