use core::future::poll_fn;
use core::task::Poll;
use stm32_metapac as pac;

pub struct MdmaCh0;

impl MdmaCh0 {
    pub fn new() -> Self {
        // Enable MDMA clock on AHB3
        pac::RCC.ahb3enr().modify(|w| w.set_mdmaen(true));
        Self
    }

    /// Generic 16-bit Memory-to-Peripheral transfer.
    /// - src_addr increments by 2 bytes (HALFWORD).
    /// - dst_addr stays fixed (FIXED).
    pub fn start_memory_to_peripheral_16bit(
        &mut self,
        src_addr: *const u16,
        dst_addr: *mut u16,
        len_halfwords: u32,
    ) {
        let ch = pac::MDMA.ch(0);

        // Clear all pending interrupt flags
        ch.ifcr().write_value(pac::mdma::regs::Ifcr(0x1F));

        ch.sar().write_value(pac::mdma::regs::Sar(src_addr as u32));
        ch.dar().write_value(pac::mdma::regs::Dar(dst_addr as u32));
        ch.bndtr().modify(|w| w.set_bndt(len_halfwords * 2));

        // TCR: SSIZE=01, SINC=01, DSIZE=01, DINC=00, TRGM=01, SWRM=1
        ch.tcr().write_value(pac::mdma::regs::Tcr(0x0141000A));

        // Enable channel and trigger software request
        ch.cr().modify(|w| w.set_en(true));
        ch.cr().modify(|w| w.set_swrq(true));
    }

    /// Yields to the Embassy executor until the transfer completes.
    pub async fn wait_complete(&mut self) {
        let ch = pac::MDMA.ch(0);

        // Hardware clears EN automatically when the transfer finishes.
        while ch.cr().read().en() {
            yield_now().await;
        }

        // Clear transfer complete flags
        ch.ifcr().write_value(pac::mdma::regs::Ifcr(0x1F));
    }
}

pub async fn yield_now() {
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }).await;
}
