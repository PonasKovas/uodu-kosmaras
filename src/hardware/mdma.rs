use core::future::poll_fn;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Poll;
use embassy_stm32::interrupt;
use embassy_stm32::interrupt::typelevel::Interrupt;
use embassy_sync::waitqueue::AtomicWaker;
use stm32_metapac as pac;
use stm32_metapac::mdma::vals;

static MDMA_WAKER: AtomicWaker = AtomicWaker::new();
/// Set by the ISR when a transfer completes, cleared by `start_*` before enabling the channel.
static MDMA_DONE: AtomicBool = AtomicBool::new(false);

pub struct MdmaInterruptHandler;

impl interrupt::typelevel::Handler<interrupt::typelevel::MDMA> for MdmaInterruptHandler {
    unsafe fn on_interrupt() {
        let ch = pac::MDMA.ch(0);

        // Clear the channel transfer complete flag. CTCIE stays enabled, because
        // disabling it here could race with the next transfer being armed.
        ch.ifcr().write(|w| w.set_cctcif(true));
        // Read back so the flag clear has landed before the ISR returns
        // (avoids a spurious immediate re-entry from a posted write).
        let _ = ch.cr().read();

        MDMA_DONE.store(true, Ordering::Release);
        MDMA_WAKER.wake();
    }
}

pub struct MdmaCh0;

impl MdmaCh0 {
    pub fn new(
        _irq: impl interrupt::typelevel::Binding<interrupt::typelevel::MDMA, MdmaInterruptHandler>,
    ) -> Self {
        pac::RCC.ahb3enr().modify(|w| w.set_mdmaen(true));

        // Safely enable the MDMA interrupt in the NVIC.
        // The unsafe block is required by the HAL, but it is architecturally
        // safe here because the _irq binding ensures a valid handler exists.
        unsafe {
            interrupt::typelevel::MDMA::enable();
        }

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

        MDMA_DONE.store(false, Ordering::Release);

        // Clear all pending interrupt flags
        ch.ifcr().write(|w| {
            w.set_cteif(true);
            w.set_cctcif(true);
            w.set_cbrtif(true);
            w.set_cbtif(true);
            w.set_cltcif(true);
        });

        ch.sar().write_value(pac::mdma::regs::Sar(src_addr as u32));
        ch.dar().write_value(pac::mdma::regs::Dar(dst_addr as u32));
        ch.bndtr().modify(|w| w.set_bndt(len_halfwords * 2));

        ch.tcr().write(|w| {
            w.set_sinc(vals::Incmode::INCREMENT);
            w.set_dinc(vals::Incmode::FIXED);
            w.set_ssize(vals::Wordsize::HALF_WORD);
            w.set_dsize(vals::Wordsize::HALF_WORD);
            w.set_sincos(vals::Wordsize::HALF_WORD); // source steps 2 bytes per transfer
            w.set_dincos(vals::Wordsize::HALF_WORD); // ignored with DINC=FIXED
            w.set_sburst(vals::Burst::INCR16); // 16 beats x 2 B = 32 B per source burst
            w.set_dburst(vals::Burst::SINGLE);
            w.set_trgm(vals::Trgm::BLOCK);
            // TLEN represents (N + 1) bytes. 63 = 64 bytes per buffer.
            // 960 byte line / 64 = exactly 15 chunks.
            w.set_tlen(63);
            w.set_swrm(true);
        });

        // Enable channel, interrupt, and trigger software request
        ch.cr().modify(|w| {
            w.set_ctcie(true);
            w.set_en(true);
            w.set_swrq(true);
        });
    }

    /// Yields to the Embassy executor until the transfer's completion ISR has run.
    pub async fn wait_complete(&mut self) {
        poll_fn(|cx| {
            MDMA_WAKER.register(cx.waker());

            if MDMA_DONE.load(Ordering::Acquire) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}
