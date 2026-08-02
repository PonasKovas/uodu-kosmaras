use embassy_stm32::dac::{Ch2, DacChannel};
use embassy_stm32::mode::Blocking;
use embassy_stm32::peripherals::{DAC1, OPAMP2, PE7, PE8};
use embassy_stm32::Peri;
use stm32_metapac as pac;
use stm32_metapac::opamp::vals::{VmSel, VpSel};

/// Helper function to perform integer division with nearest-neighbor rounding.
const fn round_div(numerator: u32, denominator: u32) -> u32 {
    (numerator + denominator / 2) / denominator
}

/// Converts a voltage target in millivolts to a 12-bit DAC value (0..=4095).
const fn mv_to_dac_12bit(mv: u32, vref_mv: u32) -> u16 {
    const MAX_DAC_VALUE: u32 = 4095;
    round_div(mv * MAX_DAC_VALUE, vref_mv) as u16
}

#[embassy_executor::task]
pub async fn regulator_task(
    _pe7: Peri<'static, PE7>,
    _pe8: Peri<'static, PE8>,
    dac1: Peri<'static, DAC1>,
    _opamp2: Peri<'static, OPAMP2>,
) {
    // ------------------------------------------------------------------------
    // 1. Configure PE7 (VOUT) and PE8 (VINM) in Analog Mode via PAC
    // ------------------------------------------------------------------------
    // Enable GPIOE clock on AHB4 bus
    pac::RCC.ahb4enr().modify(|w| {
        w.set_gpioeen(true);
    });

    // Set PE7 and PE8 pin modes to Analog
    pac::GPIOE.moder().modify(|w| {
        w.set_moder(7, pac::gpio::vals::Moder::ANALOG);
        w.set_moder(8, pac::gpio::vals::Moder::ANALOG);
    });

    // ------------------------------------------------------------------------
    // 2. Configure DAC1 Channel 2 (Index 1) for Internal Routing
    // ------------------------------------------------------------------------
    const TARGET_MV: u32 = 47;
    const VREF_MV: u32 = 3300;
    const DAC_VALUE: u16 = mv_to_dac_12bit(TARGET_MV, VREF_MV);

    let mut dac_ch2: DacChannel<'static, Blocking> =
        DacChannel::new_internal_blocking::<DAC1, Ch2>(dac1);
    dac_ch2.set(embassy_stm32::dac::Value::Bit12Right(DAC_VALUE));

    // ------------------------------------------------------------------------
    // 3. Configure OPAMP2 via PAC
    // ------------------------------------------------------------------------
    // OPAMP peripheral clock is located on the APB1H (APB1 High) bus on STM32H7
    pac::RCC.apb1henr().modify(|w| {
        w.set_opampen(true);
    });

    // Configure OPAMP2 Control & Status Register (OPAMP2_CSR):
    // - set_opampen: true -> Enable OPAMP2 hardware
    pac::OPAMP2.csr().modify(|w| {
        w.set_vp_sel(VpSel::DAC_OUT);
        w.set_vm_sel(VmSel::INM0);
        w.set_opampen(true);
    });

    core::future::pending::<()>().await;
}
