use defmt::{error, info};
use embassy_executor::task;
use embassy_time::{with_timeout, Duration, Timer};

use crate::{hardware::pssi::Pssi16, peer_sync};

const N: usize = 256;

#[repr(C, align(4))]
struct Frame([u16; N]);

static mut M1_TX: Frame = Frame([0; N]);
static mut M0_RX: Frame = Frame([0; N]);

/// M1: sends a counting pattern, one frame every 100 ms.
#[task]
pub async fn m1_tx_task(mut pssi: Pssi16<'static>) {
    peer_sync::wait_peer_ready().await;
    let buf = unsafe { &mut (*core::ptr::addr_of_mut!(M1_TX)).0 };
    let mut base: u16 = 0;
    loop {
        for (i, v) in buf.iter_mut().enumerate() {
            *v = base.wrapping_add(i as u16);
        }
        match with_timeout(Duration::from_secs(1), pssi.write(buf)).await {
            Ok(Ok(())) => info!("tx frame base={}", base),
            Ok(Err(e)) => error!("tx error: {}", e),
            Err(_) => error!("tx timeout (no RDY from receiver?)"),
        }
        base = base.wrapping_add(1);
        Timer::after_millis(100).await;
    }
}

/// M0: receives frames and checks the counting pattern.
#[task]
pub async fn m0_rx_task(mut pssi: Pssi16<'static>) {
    peer_sync::wait_peer_ready().await;

    let buf = unsafe { &mut (*core::ptr::addr_of_mut!(M0_RX)).0 };
    loop {
        buf.fill(0);
        match with_timeout(Duration::from_secs(1), pssi.read(buf)).await {
            Ok(Ok(())) => {
                let base = buf[0];
                let bad = buf
                    .iter()
                    .enumerate()
                    .filter(|(i, v)| **v != base.wrapping_add(*i as u16))
                    .count();
                if bad == 0 {
                    info!("rx frame ok base={}", base);
                } else {
                    error!(
                        "rx frame base={} mismatches={}/{} head={=u16:x} {=u16:x} {=u16:x} {=u16:x} tail={=u16:x} {=u16:x}",
                        base,
                        bad,
                        N,
                        buf[0],
                        buf[1],
                        buf[2],
                        buf[3],
                        buf[N - 2],
                        buf[N - 1]
                    );
                }
            }
            Ok(Err(e)) => error!("rx error: {}", e),
            Err(_) => error!("rx timeout (no DE/data from sender?)"),
        }
    }
}
