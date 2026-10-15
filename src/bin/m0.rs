#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::dfsdm::config::{CkoutDivider, CkoutSource, InternalSpiMode};
use embassy_stm32::dfsdm::FilterConfig;
use embassy_stm32::peripherals::DFSDM1;
use embassy_stm32::time::Hertz;
use panic_probe as _;

use uodu_kosmaras::hardware::mpu_conf::configure_mpu;
use uodu_kosmaras::hardware::rcc::rcc_config;
use uodu_kosmaras::hardware::st7796::St7796;
use uodu_kosmaras::{heartbeat, power, verify_hardware_uid};

embassy_stm32::bind_interrupts!(struct MdmaIrqs {
    MDMA => uodu_kosmaras::hardware::mdma::MdmaInterruptHandler;
});

const EXPECTED_M0_UID: [u8; 12] = [
    0x33, 0x0, 0x10, 0x0, 0x19, 0x51, 0x31, 0x32, 0x34, 0x35, 0x32, 0x30,
];

// Allocate the DMA ring buffer.
static mut AUDIO_DMA_BUF: [u32; 512] = [0; 512];

// Allocate the chunk buffer statically to prevent the `main` task future
// from blowing up the executor's 4KB arena!
static mut AUDIO_CHUNK: [u32; 256] = [0; 256];

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    verify_hardware_uid(EXPECTED_M0_UID, "M0");
    info!("Booting M0");

    let mut config = embassy_stm32::Config::default();
    config.enable_debug_during_sleep = true;
    config.rcc = rcc_config();

    let p = embassy_stm32::init(config);
    let mut core_peri = cortex_m::Peripherals::take().unwrap();

    let dma_buf_addr = core::ptr::addr_of!(uodu_kosmaras::plasma::DMA_BUFFERS) as u32;
    configure_mpu(core_peri.MPU, true, Some(dma_buf_addr));
    core_peri.SCB.enable_icache();

    spawner
        .spawn(heartbeat::heartbeat_task(p.PE3.into()))
        .unwrap();

    spawner
        .spawn(power::power_task(p.PG1, p.PG0, p.EXTI0))
        .unwrap();

    let display = St7796::init(
        p.FMC, p.PD14, p.PD15, p.PD0, p.PD1, p.PE7, p.PE8, p.PE9, p.PE10, p.PE11, p.PE12, p.PE13,
        p.PE14, p.PE15, p.PD8, p.PD9, p.PD10, p.PD7, p.PE6, p.PD5, p.PD4, p.PD11,
    )
    .await;

    let mdma = uodu_kosmaras::hardware::mdma::MdmaCh0::new(MdmaIrqs);

    spawner
        .spawn(uodu_kosmaras::plasma::plasma_task(
            display, p.PD12, p.EXTI12, mdma,
        ))
        .unwrap();

    embassy_stm32::bind_interrupts!(struct DfsdmIrqs {
        DFSDM1_FLT3 => embassy_stm32::dfsdm::filter::InterruptHandler<DFSDM1, embassy_stm32::dfsdm::types::Flt3>;
        DMA1_STREAM0 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::DMA1_CH0>;
    });

    let kernel = embassy_stm32::rcc::frequency::<DFSDM1>(); // 130 MHz
    let div = CkoutDivider::from_frequency::<DFSDM1>(kernel, Hertz(3_072_000)).unwrap();

    let dfsdm = embassy_stm32::dfsdm::Dfsdm::new_ckout(p.DFSDM1, p.PB0, CkoutSource::System, div);
    let (dfsdm_common, dfsdm) = dfsdm.configure_pins(|tb| {
        (
            tb.ch0.none(),
            tb.ch1.datin(p.PB1),
            tb.ch2.none(),
            tb.ch3.none(),
            tb.ch4.none(),
            tb.ch5.none(),
            tb.ch6.none(),
            tb.ch7.none(),
        )
    });

    let (trans, filters) = dfsdm.build(&dfsdm_common, |b| {
        b.ch1
            .build_spi_int(&dfsdm_common, InternalSpiMode::SpiFalling)
    });

    let params = embassy_stm32::dfsdm::config::FilterParameters::new(
        embassy_stm32::dfsdm::config::FilterOrder::Sinc3 { fosr: 64 },
        1,
    );
    let shift = embassy_stm32::dfsdm::config::DataRightShift::new(params.recommended_shift());

    let trans = trans.set_data_right_shift(shift).enable();

    let mut filter = filters
        .flt3
        .build(&dfsdm_common, DfsdmIrqs)
        .enable_reg_dma(
            &trans,
            [&trans],
            &FilterConfig {
                filter_params: params,
                enable_continuous_regular: true,
                enable_fast_regular: false,
                enable_regular_sync: false,
                enable_injected_sync: false,
                enable_injected_scanning: false,
                trigger: embassy_stm32::dfsdm::InjectedTrigger::Disabled,
            },
        );

    // Create the ring buffer wrapper
    let dma_buf = unsafe { &mut *core::ptr::addr_of_mut!(AUDIO_DMA_BUF) };
    let mut dma_ring = filter.regular.ring_buffered(p.DMA1_CH0, DfsdmIrqs, dma_buf);

    info!("Starting DMA audio capture...");

    // Always start the DMA engine BEFORE the peripheral it reads from to prevent instant overruns
    dma_ring.start();
    dma_ring.start_conversion();

    let chunk = unsafe { &mut *core::ptr::addr_of_mut!(AUDIO_CHUNK) };
    let mut total_samples = 0;

    let mut min = i32::MAX;
    let mut max = i32::MIN;

    loop {
        // `read` internally uses `read_exact` and returns the *remaining capacity*.
        // We ignore the returned value with `Ok(_)` because `chunk` is guaranteed to be 100% full.
        match dma_ring.read(chunk).await {
            Ok(_) => {
                for &word in chunk.iter() {
                    let sample = embassy_stm32::dfsdm::RegularResult::from_word(word).data;
                    min = min.min(sample);
                    max = max.max(sample);
                }

                total_samples += chunk.len();

                if total_samples >= 4800 {
                    info!("Audio amplitude (last {} samples): min={}, max={}", total_samples, min, max);
                    total_samples = 0;
                    min = i32::MAX;
                    max = i32::MIN;
                }
            }
            Err(e) => {
                defmt::error!("DMA Error: {}", defmt::Debug2Format(&e));
            }
        }
    }
}
