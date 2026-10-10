pub fn configure_mpu(mpu: cortex_m::peripheral::MPU) {
    unsafe {
        // Disable MPU during setup
        mpu.ctrl.write(0);

        #[cfg(feature = "m0")]
        {
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

        // Enable MPU with default memory map for other regions
        mpu.ctrl.write(1 | (1 << 2));

        // Flush CPU pipelines
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
}
