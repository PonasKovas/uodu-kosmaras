#![no_std]

pub mod current_regulator;
pub mod hardware;
pub mod heartbeat;
pub mod plasma;
pub mod power;

use defmt::info;

pub fn verify_hardware_uid(expected_uid: [u8; 12], board: &str) {
    let uid = embassy_stm32::uid::uid();

    info!("Device UID: {=[u8]:#02x}", &uid[..]);

    if *uid != expected_uid {
        defmt::panic!(
            "FATAL: {} firmware flashed onto incorrect hardware! Aborting to prevent pin damage.",
            board
        );
    }
}
