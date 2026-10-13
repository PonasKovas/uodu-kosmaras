use defmt::info;
use embassy_stm32::bind_interrupts;
use embassy_stm32::exti::{ExtiInput, InterruptHandler};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::interrupt::typelevel::EXTI0 as IrqExti0;
use embassy_stm32::peripherals::{EXTI0, PG0, PG1};
use embassy_stm32::Peri;
use embassy_time::Timer;

const DEBOUNCE_MS: u64 = 50;

bind_interrupts!(struct Irqs {
    EXTI0 => InterruptHandler<IrqExti0>;
});

#[embassy_executor::task]
pub async fn power_task(
    pwr_ctrl_pin: Peri<'static, PG1>,
    pwr_btn_pin: Peri<'static, PG0>,
    exti_ch: Peri<'static, EXTI0>,
) {
    // 1. Immediately drive PG1 (POWER_CTRL) HIGH to latch the power supply ON
    let mut pwr_ctrl = Output::new(pwr_ctrl_pin, Level::High, Speed::Low);
    info!("POWER_CTRL (PG1) set to HIGH (power supply latched ON)");

    // 2. Configure PG0 (PWR_BTN) as EXTI input with internal weak pull-up
    let mut pwr_btn = ExtiInput::new(pwr_btn_pin, exti_ch, Pull::Up, Irqs);

    // 3. If the user is holding the power button down from turning the device ON,
    // wait for them to release it first so we don't immediately shut down.
    while pwr_btn.is_low() {
        Timer::after_millis(10).await;
    }
    Timer::after_millis(DEBOUNCE_MS).await;

    info!("Power management active. Waiting for PWR_BTN (PG0) press to shut down...");

    // 4. Wait for active-LOW button press with debouncing
    loop {
        pwr_btn.wait_for_falling_edge().await;

        // Let contact bounce settle, then confirm the level is still LOW.
        // Glitches/bounces that end before the settle window are rejected.
        Timer::after_millis(DEBOUNCE_MS).await;

        if pwr_btn.is_low() {
            break;
        }
    }

    info!("PWR_BTN press detected! Initiating shutdown: setting POWER_CTRL (PG1) to LOW...");

    // 5. Release power latch to shut down the power supply
    pwr_ctrl.set_low();
}
