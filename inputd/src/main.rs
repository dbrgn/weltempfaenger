use std::process::exit;
use std::thread;
use std::time::Duration;

use ads1x1x::{channel, Ads1x1x, SlaveAddr, FullScaleRange, DataRate16Bit};
use clap::Clap;
use embedded_hal::adc::OneShot;
use linux_embedded_hal::I2cdev;
use nb::block;

#[derive(Clap, Debug)]
struct Opts {
    #[clap(default_value = "/dev/i2c-1")]
    i2c: String,
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
//fn map_potentiometer_value(val: u16) -> u8 {
//}

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
            return (
                (upper.0 - lower.0) as u32
                * (val - lower.1) as u32
                / (upper.1 - lower.1) as u32
                + lower.0 as u32
            ) as u16;
        }
    }
    MAX_ANGLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_measurement_to_angle() {
        // Min
        assert_eq!(measurement_to_angle(0), 0);

        // Max
        assert_eq!(measurement_to_angle(27000), 280);
        assert_eq!(measurement_to_angle(64000), 280);

        // Exact
        assert_eq!(measurement_to_angle(26226), 250);
        assert_eq!(measurement_to_angle(26227), 280);
        assert_eq!(measurement_to_angle(19000), 160);
        assert_eq!(measurement_to_angle(19250), 180);

        // Interpolated
        assert_eq!(measurement_to_angle(19126), 175);
    }

    #[test]
    fn test_measurement_to_angle_no_crash() {
        for i in 0..u16::MAX {
            measurement_to_angle(i);
        }
    }
}

fn main() {
    let opts: Opts = Opts::parse();

    let x = LOOKUP_TABLE_VOL[0];

    // Initialize ADC
    let dev = I2cdev::new(opts.i2c).unwrap();
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

    // Do measurement
    loop {
        let a0 = block!(adc.read(&mut channel::SingleA0)).unwrap();
        let a1 = block!(adc.read(&mut channel::SingleA1)).unwrap();
        println!("a0={} a1={}", a0, a1);
        thread::sleep(Duration::from_millis(32));
    }
}
