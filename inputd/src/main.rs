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

static LOOKUP_TABLE_VOL: [(u16, u16); 28] = [
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
    (170, 19000),
    (180, 19250),
    (190, 20080),
    (200, 21080),
    (210, 21880),
    (220, 23550),
    (230, 24680),
    (240, 25730),
    (250, 26227),
    (280, 26227),
];

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
