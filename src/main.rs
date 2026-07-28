#![no_std]
#![no_main]

mod power;

use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::rcc::*;
use embassy_stm32::time::Hertz;
use embassy_time::Timer;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    defmt::info!("--- STM32H723 High Performance 520 MHz Test ---");

    let mut config = embassy_stm32::Config::default();
    config.enable_debug_during_sleep = true;

    // Scale0 is required for high-frequency operation (> 400 MHz)
    config.rcc.voltage_scale = VoltageScale::Scale0;

    // 1. Configure 25 MHz HSE in Bypass Mode
    config.rcc.hse = Some(Hse {
        freq: Hertz(25_000_000),
        mode: HseMode::Bypass,
    });

    // 2. Configure PLL1: (25 MHz / 5) * 104 / 1 = 520 MHz SYSCLK
    config.rcc.pll1 = Some(Pll {
        source: PllSource::HSE,
        prediv: PllPreDiv::DIV5,  // 25 MHz / 5 = 5 MHz VCO Input
        mul: PllMul::MUL104,      // 5 MHz * 104 = 520 MHz VCO Output
        divp: Some(PllDiv::DIV1), // 520 MHz / 1 = 520 MHz CPU Clock
        divq: None,
        divr: None,
    });
    config.rcc.sys = Sysclk::PLL1_P; // 520 MHz CPU

    // 3. Bus Clock Prescalers (AHB max 275 MHz, APB max 137.5 MHz)
    config.rcc.ahb_pre = AHBPrescaler::DIV2; // 520 / 2 = 260 MHz
    config.rcc.apb1_pre = APBPrescaler::DIV2; // 260 / 2 = 130 MHz
    config.rcc.apb2_pre = APBPrescaler::DIV2; // 260 / 2 = 130 MHz
    config.rcc.apb3_pre = APBPrescaler::DIV2; // 260 / 2 = 130 MHz
    config.rcc.apb4_pre = APBPrescaler::DIV2; // 260 / 2 = 130 MHz

    let p = embassy_stm32::init(config);

    // Spawn the background power management task
    //spawner.spawn(power::power_task(p.PG1, p.PG0)).unwrap();

    let mut pe3 = Output::new(p.PE3, Level::High, Speed::Low);

    defmt::info!("SUCCESS! CPU running rock-solid at 520 MHz!");

    loop {
        pe3.toggle();
        Timer::after_secs(1).await;
    }
}
