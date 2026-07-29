use embassy_stm32::gpio::{AnyPin, Level, Output, Speed};
use embassy_stm32::Peri;
use embassy_time::Timer;

#[embassy_executor::task]
pub async fn heartbeat_task(pin: Peri<'static, AnyPin>) {
	let mut out = Output::new(pin, Level::High, Speed::Low);

	loop {
        out.toggle();

        Timer::after_millis(250).await;
    }
}
