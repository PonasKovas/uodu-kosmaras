use defmt::info;
use embassy_stm32::gpio::{Input, Level, Output, Pull, Speed};
use embassy_stm32::peripherals::{PG0, PG1};
use embassy_stm32::Peri;
use embassy_time::Timer;

#[embassy_executor::task]
pub async fn power_task(pwr_ctrl_pin: Peri<'static, PG1>, pwr_btn_pin: Peri<'static, PG0>) {
    // 1. Immediately drive PG1 (POWER_CTRL) HIGH to latch the power supply ON
    let mut pwr_ctrl = Output::new(pwr_ctrl_pin, Level::High, Speed::Low);
    info!("POWER_CTRL (PG1) set to HIGH (power supply latched ON)");

    // 2. Configure PG0 (PWR_BTN) as input with internal weak pull-up
    let pwr_btn = Input::new(pwr_btn_pin, Pull::Up);

    // 3. If the user is holding the power button down from turning the device ON,
    // wait for them to release it first so we don't immediately shut down.
    while pwr_btn.is_low() {
        Timer::after_millis(10).await;
    }

    info!("Power management active. Waiting for PWR_BTN (PG0) press to shut down...");

    // 4. Wait for active-LOW button press
    while pwr_btn.is_high() {
        Timer::after_millis(10).await;
    }

    info!("PWR_BTN press detected! Initiating shutdown: setting POWER_CTRL (PG1) to LOW...");

    // 5. Release power latch to shut down the power supply
    pwr_ctrl.set_low();
}
