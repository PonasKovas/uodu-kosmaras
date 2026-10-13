use embassy_stm32::rcc::*;
use embassy_stm32::time::Hertz;

pub fn rcc_config() -> embassy_stm32::rcc::Config {
    let mut rcc = embassy_stm32::rcc::Config::new();

    // Scale0 is required for high-frequency operation (> 400 MHz)
    rcc.voltage_scale = VoltageScale::Scale0;

    // 1. Configure 25 MHz HSE in Bypass Mode
    rcc.hse = Some(Hse {
        freq: Hertz(25_000_000),
        mode: HseMode::Bypass,
    });

    // 2. Configure PLL1: (25 MHz / 5) * 104 / 1 = 520 MHz SYSCLK
    rcc.pll1 = Some(Pll {
        source: PllSource::Hse,
        prediv: PllPreDiv::Div5,  // 25 MHz / 5 = 5 MHz VCO Input
        mul: PllMul::Mul104,      // 5 MHz * 104 = 520 MHz VCO Output
        divp: Some(PllDiv::Div1), // 520 MHz / 1 = 520 MHz CPU Clock
        divq: None,
        divr: None,
    });
    rcc.sys = Sysclk::Pll1P; // 520 MHz CPU

    // 3. Bus Clock Prescalers (AHB max 275 MHz, APB max 137.5 MHz)
    rcc.ahb_pre = AHBPrescaler::Div2; // 520 / 2 = 260 MHz
    rcc.apb1_pre = APBPrescaler::Div2; // 260 / 2 = 130 MHz
    rcc.apb2_pre = APBPrescaler::Div2; // 260 / 2 = 130 MHz
    rcc.apb3_pre = APBPrescaler::Div2; // 260 / 2 = 130 MHz
    rcc.apb4_pre = APBPrescaler::Div2; // 260 / 2 = 130 MHz

    rcc
}
