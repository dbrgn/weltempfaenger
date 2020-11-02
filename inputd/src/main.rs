use std::{
    process::{exit, Command, Stdio},
    thread,
    time::Duration,
};

use ads1x1x::{channel, Ads1x1x, DataRate16Bit, FullScaleRange, SlaveAddr};
use clap::Clap;
use embedded_hal::adc::OneShot;
use linux_embedded_hal::I2cdev;
use nb::block;

#[cfg(test)]
mod tests;

#[derive(Clap, Debug, Clone)]
struct Opts {
    #[clap(default_value = "/dev/i2c-1")]
    i2c: String,
    #[clap(default_value = "volumio")]
    volumio_command: String,
}

const LOOKUP_TABLE_VOL: [(u16, u16); 28] = [
    // (angle, value)
    (0, 10),
    (10, 20),
    (20, 280),
    (25, 1200),
    (30, 2600),
    (40, 4700),
    (50, 7500),
    (60, 10000),
    (70, 13500),
    (80, 14900),
    (90, 15800),
    (100, 16600),
    (110, 17200),
    (120, 17700),
    (130, 18400),
    (140, 18700),
    (150, 18800),
    (160, 19000),
    (170, 19002),
    (180, 19250),
    (190, 20080),
    (200, 21082),
    (210, 21880),
    (220, 23550),
    (230, 24680),
    (240, 25730),
    (250, 26226),
    (280, 26227),
];
const MIN_ANGLE: u16 = LOOKUP_TABLE_VOL[0].0;
const MAX_ANGLE: u16 = LOOKUP_TABLE_VOL[LOOKUP_TABLE_VOL.len() - 1].0;
const MIN_VALUE: u16 = LOOKUP_TABLE_VOL[0].1;
const MAX_VALUE: u16 = LOOKUP_TABLE_VOL[LOOKUP_TABLE_VOL.len() - 1].1;

/// Convert a 12-bit input measurement to a value between 0 and 100.
fn map_potentiometer_value(val: u16) -> u8 {
    let angle = measurement_to_angle(val);
    let percent = (angle - MIN_ANGLE) * 100 / (MAX_ANGLE - MIN_ANGLE);
    assert!(percent <= 100);
    100 - percent as u8
}

fn measurement_to_angle(val: u16) -> u16 {
    // Lower and upper bounds
    if val <= MIN_VALUE {
        return MIN_ANGLE;
    }
    if val >= MAX_VALUE {
        return MAX_ANGLE;
    }

    for i in 0..LOOKUP_TABLE_VOL.len() {
        if LOOKUP_TABLE_VOL[i].1 == val {
            // We found an exact match
            return LOOKUP_TABLE_VOL[i].0;
        } else if LOOKUP_TABLE_VOL[i].1 > val {
            // The measurement is between the previous and the current entry.
            let lower = LOOKUP_TABLE_VOL[i - 1];
            let upper = LOOKUP_TABLE_VOL[i];

            // Interpolate between the two angles.
            return ((upper.0 - lower.0) as u32 * (val - lower.1) as u32
                / (upper.1 - lower.1) as u32
                + lower.0 as u32) as u16;
        }
    }
    MAX_ANGLE
}

/// Set the volume using the volumio command with the specified name.
fn set_volume(cmd: &str, volume: u8) {
    // Clamp volume to 0-100
    let volume = std::cmp::min(volume, 100);

    // Set volume
    let status_res = Command::new(cmd)
        .arg("volume")
        .arg(&volume.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status_res {
        Ok(status) if status.success() => println!("Set volume to {}%", volume),
        Ok(status) => eprintln!("Error: Exit status {} when setting volume", status),
        Err(e) => eprintln!("Error: Could not set volume: {}", e),
    };
}

type Adc = Ads1x1x<
    ads1x1x::interface::I2cInterface<linux_embedded_hal::I2cdev>,
    ads1x1x::ic::Ads1115,
    ads1x1x::ic::Resolution16Bit,
    ads1x1x::mode::OneShot,
>;

fn adc_loop(mut adc: Adc, opts: Opts) -> ! {
    // Do measurement
    loop {
        // Analog input 0 ("Lautstärke")
        let a0 = block!(adc.read(&mut channel::SingleA0)).unwrap();
        let volume = map_potentiometer_value(a0 as u16);

        // Analog input 1 ("Klangfarbe")
        let a1 = block!(adc.read(&mut channel::SingleA1)).unwrap();

        // Print values
        println!("a0={} a1={} vol={}", a0, a1, volume);

        // Set volume
        set_volume(&opts.volumio_command, volume);

        // Sleep for some milliseconds
        thread::sleep(Duration::from_millis(250));
    }
}

fn main() {
    let opts: Opts = Opts::parse();

    // Initialize ADC
    let dev = I2cdev::new(&opts.i2c).unwrap();
    let address = SlaveAddr::default();
    let mut adc = Ads1x1x::new_ads1115(dev, address);

    // Configure PGA (gain)
    if let Err(e) = adc.set_full_scale_range(FullScaleRange::Within4_096V) {
        eprintln!("Could not set full scale range: {:?}", e);
        exit(1);
    }

    // Configure sample rate
    if let Err(e) = adc.set_data_rate(DataRate16Bit::Sps32) {
        eprintln!("Warning: Could not set data rate: {:?}", e);
    }

    adc_loop(adc, opts.clone());
}
