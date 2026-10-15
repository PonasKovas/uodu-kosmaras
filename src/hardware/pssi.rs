//! 16-bit PSSI driver (register level via metapac, DMA2_CH0 via embassy).
//!
//! The PDCK clock is produced by TIM2_CH4 on PA3 and is only started if a
//! `ClockOut` is passed to `Pssi16::new` (M0). M1 passes `None`. PA3 is wired
//! externally to PDCK (PA6) on both MCUs.
//!
//! Direction is per call: `read()` receives, `write()` transmits. Between calls
//! PSSI is disabled. Both sides must agree on the transfer length.
//! The TX side drives DE, the RX side drives RDY.

use embassy_stm32::dma::{self, Channel, Request, TransferOptions};
use embassy_stm32::gpio::OutputType;
use embassy_stm32::peripherals::{DMA2_CH0, PA3, PSSI, TIM2};
use embassy_stm32::pssi::{
    D0Pin, D10Pin, D11Pin, D12Pin, D13Pin, D14Pin, D15Pin, D1Pin, D2Pin, D3Pin, D4Pin, D5Pin,
    D6Pin, D7Pin, D8Pin, D9Pin, DePin, Dma, PdckPin, RdyPin,
};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer::low_level::CountingMode;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_stm32::{bind_interrupts, Peri};
use embassy_time::Timer;
use stm32_metapac as pac;
use stm32_metapac::gpio::vals::{Idr, Moder, Ospeedr, Ot, Pupdr};
use stm32_metapac::pssi::regs::{Cr, Icr};
use stm32_metapac::pssi::vals;

bind_interrupts!(struct DmaIrqs {
    DMA2_STREAM0 => dma::InterruptHandler<DMA2_CH0>;
});

/// DMAMUX1 request number for PSSI (shared with DCMI on STM32H723).
const PSSI_DMA_REQUEST: Request = 75;

#[derive(Debug, defmt::Format)]
pub enum Error {
    /// Buffer must be 4-byte aligned and have an even number of u16 elements.
    BadBuffer,
    /// FIFO overrun/underrun was flagged during the transfer.
    Overrun,
}

pub struct Config {
    /// PDCK frequency. Generated on M0, used for TX drain timing on both.
    pub clock: Hertz,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            clock: Hertz(1_000_000),
        }
    }
}

/// Resources for generating PDCK. Pass `Some` on the clock master only.
pub struct ClockOut<'d> {
    pub tim: Peri<'d, TIM2>,
    pub pin: Peri<'d, PA3>,
}

pub struct Pssi16<'d> {
    dma: Channel<'d>,
    request: Request,
    clock_hz: u32,
	rdy: RdyLine,
    _pwm: Option<SimplePwm<'d, TIM2>>,
}

fn gpio(port: u8) -> pac::gpio::Gpio {
    // H7 GPIO ports: base 0x5802_0000, stride 0x400.
    unsafe { pac::gpio::Gpio::from_ptr((0x5802_0000usize + 0x400 * port as usize) as *mut ()) }
}

fn set_af(port: u8, pin: u8, af: u8, pull: Pupdr) {
    let pin = pin as usize;
    let gpio = gpio(port);
    gpio.moder().modify(|w| w.set_moder(pin, Moder::ALTERNATE));
    gpio.otyper().modify(|w| w.set_ot(pin, Ot::PUSH_PULL));
    gpio.ospeedr()
        .modify(|w| w.set_ospeedr(pin, Ospeedr::VERY_HIGH_SPEED));
    gpio.pupdr().modify(|w| w.set_pupdr(pin, pull));
    gpio.afr(pin / 8).modify(|w| w.set_afr(pin % 8, af));
}

/// Location of the RDY pin, so TX can sample the receiver's RDY as a plain GPIO.
#[derive(Clone, Copy)]
struct RdyLine {
    port: u8,
    pin: u8,
    af: u8,
}

/// Switches the RDY pin to plain input while alive; restores AF mode on drop
/// (also when the surrounding future is cancelled).
struct RdyAsInput(RdyLine);

impl RdyAsInput {
    fn new(line: RdyLine) -> Self {
        gpio(line.port)
            .moder()
            .modify(|w| w.set_moder(line.pin as usize, Moder::INPUT));
        Self(line)
    }

    fn is_high(&self) -> bool {
        gpio(self.0.port).idr().read().idr(self.0.pin as usize) == Idr::HIGH
    }
}

impl Drop for RdyAsInput {
    fn drop(&mut self) {
        set_af(self.0.port, self.0.pin, self.0.af, Pupdr::PULL_DOWN);
    }
}

macro_rules! af {
    ($pin:expr, $pull:expr) => {
        set_af($pin.port(), $pin.pin(), $pin.af_num(), $pull)
    };
}

/// Disables PSSI when dropped, so a cancelled future leaves the bus idle.
struct DisableOnDrop;

impl Drop for DisableOnDrop {
    fn drop(&mut self) {
        pac::PSSI.cr().modify(|w| {
            w.set_dmaen(false);
            w.set_enable(false);
        });
    }
}

fn cr_value(transmit: bool, run: bool) -> Cr {
    let mut cr = Cr(0);
    cr.set_ckpol(vals::Ckpol::RISING_EDGE);
    cr.set_depol(vals::Depol::ACTIVE_HIGH);
    cr.set_rdypol(vals::Rdypol::ACTIVE_HIGH);
    cr.set_edm(vals::Edm::BIT_WIDTH16);
    cr.set_derdycfg(vals::Derdycfg::RDY_DE_ALT);
    cr.set_outen(if transmit {
        vals::Outen::TRANSMIT_MODE
    } else {
        vals::Outen::RECEIVE_MODE
    });
    cr.set_dmaen(run);
    cr.set_enable(run);
    cr
}

fn clear_overrun() {
    let mut icr = Icr(0);
    icr.set_ovr_isc(true);
    pac::PSSI.icr().write_value(icr);
}

fn check_overrun() -> Result<(), Error> {
    if pac::PSSI.ris().read().ovr_ris() {
        clear_overrun();
        Err(Error::Overrun)
    } else {
        Ok(())
    }
}

impl<'d> Pssi16<'d> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        _pssi: Peri<'d, PSSI>,
        dma: Peri<'d, DMA2_CH0>,
        clock_out: Option<ClockOut<'d>>,
        pdck: Peri<'d, impl PdckPin<PSSI>>,
        de: Peri<'d, impl DePin<PSSI>>,
        rdy: Peri<'d, impl RdyPin<PSSI>>,
        d0: Peri<'d, impl D0Pin<PSSI>>,
        d1: Peri<'d, impl D1Pin<PSSI>>,
        d2: Peri<'d, impl D2Pin<PSSI>>,
        d3: Peri<'d, impl D3Pin<PSSI>>,
        d4: Peri<'d, impl D4Pin<PSSI>>,
        d5: Peri<'d, impl D5Pin<PSSI>>,
        d6: Peri<'d, impl D6Pin<PSSI>>,
        d7: Peri<'d, impl D7Pin<PSSI>>,
        d8: Peri<'d, impl D8Pin<PSSI>>,
        d9: Peri<'d, impl D9Pin<PSSI>>,
        d10: Peri<'d, impl D10Pin<PSSI>>,
        d11: Peri<'d, impl D11Pin<PSSI>>,
        d12: Peri<'d, impl D12Pin<PSSI>>,
        d13: Peri<'d, impl D13Pin<PSSI>>,
        d14: Peri<'d, impl D14Pin<PSSI>>,
        d15: Peri<'d, impl D15Pin<PSSI>>,
        config: Config,
    ) -> Self {
                embassy_stm32::rcc::enable_and_reset::<PSSI>();
        // PSSI shares the DCMI clock enable bit (AHB2ENR.DCMIEN).
        pac::RCC.ahb2enr().modify(|w| w.set_dcmien(true));

		let rdy_line = RdyLine {
            port: rdy.port(),
            pin: rdy.pin(),
            af: rdy.af_num(),
        };

        af!(pdck, Pupdr::FLOATING);
        af!(de, Pupdr::PULL_DOWN);
        af!(rdy, Pupdr::PULL_DOWN);
        af!(d0, Pupdr::FLOATING);
        af!(d1, Pupdr::FLOATING);
        af!(d2, Pupdr::FLOATING);
        af!(d3, Pupdr::FLOATING);
        af!(d4, Pupdr::FLOATING);
        af!(d5, Pupdr::FLOATING);
        af!(d6, Pupdr::FLOATING);
        af!(d7, Pupdr::FLOATING);
        af!(d8, Pupdr::FLOATING);
        af!(d9, Pupdr::FLOATING);
        af!(d10, Pupdr::FLOATING);
        af!(d11, Pupdr::FLOATING);
        af!(d12, Pupdr::FLOATING);
        af!(d13, Pupdr::FLOATING);
        af!(d14, Pupdr::FLOATING);
        af!(d15, Pupdr::FLOATING);

        pac::PSSI.cr().write_value(cr_value(false, false));
        clear_overrun();

        let request = PSSI_DMA_REQUEST;
        let dma = Channel::new(dma, DmaIrqs);

        // Start the clock last, once everything else is configured.
        let pwm = clock_out.map(|c| {
            let ch4 = PwmPin::new(c.pin, OutputType::PushPull);
            let mut pwm = SimplePwm::new(
                c.tim,
                None,
                None,
                None,
                Some(ch4),
                config.clock,
                CountingMode::EdgeAlignedUp,
            );
            {let mut ch = pwm.ch4();
            ch.set_duty_cycle_fraction(1, 2);
            ch.enable()};
            pwm
        });

        Self {
            dma,
            request,
            clock_hz: config.clock.0,
			rdy: rdy_line,
            _pwm: pwm,
        }
    }

    /// Receives `buf.len()` samples. Asserts RDY once armed. Wrap in a
    /// timeout if the sender may not show up; dropping the future is safe.
    pub async fn read(&mut self, buf: &mut [u16]) -> Result<(), Error> {
        if buf.as_ptr() as usize % 4 != 0 || buf.len() % 2 != 0 {
            return Err(Error::BadBuffer);
        }
        let words =
            unsafe { core::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u32, buf.len() / 2) };

        pac::PSSI.cr().write_value(cr_value(false, false));
        clear_overrun();

        let _guard = DisableOnDrop; // declared first, so it drops after the transfer
        let transfer = unsafe {
            self.dma.read(
                self.request,
                pac::PSSI.dr().as_ptr() as *mut u32,
                words,
                TransferOptions::default(),
            )
        };
        pac::PSSI.cr().write_value(cr_value(false, true));
        transfer.await;

        check_overrun()
    }

    /// Transmits `buf`. Waits for the receiver's RDY. Wrap in a timeout if
    /// the receiver may not show up; dropping the future is safe.
    pub async fn write(&mut self, buf: &[u16]) -> Result<(), Error> {
        if buf.as_ptr() as usize % 4 != 0 || buf.len() % 2 != 0 {
            return Err(Error::BadBuffer);
        }
        let words =
            unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u32, buf.len() / 2) };

        pac::PSSI.cr().write_value(cr_value(true, false));
        clear_overrun();

        let _guard = DisableOnDrop;
        let transfer = unsafe {
            self.dma.write(
                self.request,
                words,
                pac::PSSI.dr().as_ptr() as *mut u32,
                TransferOptions::default(),
            )
        };
                // Don't enable TX until the receiver's RDY is already high. Enabling
        // while RDY rises corrupts the first sample (observed: frame shifted by one).
        {
            let rdy = RdyAsInput::new(self.rdy);
            while !rdy.is_high() {
                Timer::after_micros(20).await;
            }
        }
        pac::PSSI.cr().write_value(cr_value(true, true));
        transfer.await;

        // DMA is done once the last word is in the FIFO, not once it is on the
        // wire. Give the FIFO ~16 PDCK periods to drain before disabling.
        Timer::after_micros(16_000_000 / self.clock_hz as u64 + 2).await;

        check_overrun()
    }
}
