//! Boot/reset synchronisation between M0 and M1 over the READY_MCUx lines.
//!
//! Each MCU drives its own READY line high once firmware is running and watches
//! the peer's line. Peer line low for 3 ms after it was seen high means the peer
//! reset, so this MCU resets too and both come back up together.

use core::sync::atomic::{AtomicBool, Ordering};

use defmt::info;
use embassy_stm32::gpio::{AnyPin, Input, Level, Output, Pull, Speed};
use embassy_stm32::Peri;
use embassy_time::Timer;

static PEER_READY: AtomicBool = AtomicBool::new(false);

/// Resolves once the peer's READY line has been seen high.
pub async fn wait_peer_ready() {
    while !PEER_READY.load(Ordering::Acquire) {
        Timer::after_millis(1).await;
    }
}

/// M0: own = PG2, peer = PG3. M1: own = PG3, peer = PG2.
#[embassy_executor::task]
pub async fn peer_sync_task(own: Peri<'static, AnyPin>, peer: Peri<'static, AnyPin>) {
    let mut own = Output::new(own, Level::Low, Speed::Low);
    let peer = Input::new(peer, Pull::Down);

    own.set_high();

    let mut low_ms = 0u8;
    loop {
        Timer::after_millis(1).await;
        let high = peer.is_high();

        if !PEER_READY.load(Ordering::Relaxed) {
            if high {
                PEER_READY.store(true, Ordering::Release);
                info!("peer up");
            }
        } else if high {
            low_ms = 0;
        } else {
            low_ms += 1;
            if low_ms >= 3 {
                info!("peer lost, resetting");
                cortex_m::peripheral::SCB::sys_reset();
            }
        }
    }
}
