pub fn configure_mpu(
    mpu: cortex_m::peripheral::MPU,
    fmc_region: bool,
    dma_buffer_addr: Option<u32>,
) {
    unsafe {
        // Disable MPU during setup
        mpu.ctrl.write(0);

        if fmc_region {
            // Configure Region 0 for FMC Bank 1
            mpu.rnr.write(0);
            mpu.rbar.write(0x6000_0000);

            // Set to Device Memory (Strongly Ordered, Non-Cacheable)
            // Size = 256MB, AP = Full Access, XN = Execute Never
            mpu.rasr.write(
                (1 << 28) | // XN
            (3 << 24) | // AP
            (1 << 16) | // B (Device Memory)
            (27 << 1) | // Size (256MB)
            1, // Enable
            );
        }

        if let Some(addr) = dma_buffer_addr {
            // Configure Region 1 for the DMA Buffer (2 KB)
            mpu.rnr.write(1);
            mpu.rbar.write(addr);

            // Normal Memory, Non-Cacheable, Shared
            // Size = 2KB, AP = Full Access, XN = Execute Never
            mpu.rasr.write(
                (1 << 28) | // XN
                (3 << 24) | // AP
                (1 << 19) | // TEX=001 (Normal memory)
                (1 << 18) | // S=1 (Shared)
                (0 << 17) | // C=0 (Non-cacheable)
                (0 << 16) | // B=0
                (10 << 1) | // Size (2KB)
                1, // Enable
            );
        }

        // Enable MPU with default memory map for other regions
        mpu.ctrl.write(1 | (1 << 2));

        // Flush CPU pipelines
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
}
