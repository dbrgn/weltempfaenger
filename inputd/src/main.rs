use std::{
    process::{exit, Command, Stdio},
    thread,
    time::Duration,
};

use ads1x1x::{channel, Ads1x1x, DataRate16Bit, FullScaleRange, SlaveAddr};
use clap::Clap;
use debouncr::{debounce_stateful_16, DebouncerStateful, Edge, Repeat16};
use embedded_hal::adc::OneShot;
use linux_embedded_hal::I2cdev;
use nb::block;
use rppal::gpio::{Gpio, InputPin, Level};

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

/// Wait for volumio to be started.
fn wait_for_volumio(cmd: &str) {
    loop {
        // To test whether volumio is working, try setting the volume to "30".
        let status_res = Command::new(cmd)
            .arg("volume")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status_res {
            Ok(status) if status.success() => {
                println!("Volumio is ready!");
                return;
            },
            Ok(status) => eprintln!("Waiting for volumio, exit status {}", status),
            Err(e) => eprintln!("Waiting for volumio, {}", e),
        };
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
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

/// Play a playlist through the API.
fn play_playlist(name: &str) {
    let status_res = Command::new("/usr/bin/curl")
        .arg(format!("http://127.0.0.1:3000/api/v1/commands/?cmd=playplaylist&name={}", name))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status_res {
        Ok(status) if status.success() => println!("Started playlist {}", name),
        Ok(status) => eprintln!("Error: Exit status {} when starting playlist {}", status, name),
        Err(e) => eprintln!("Error: Could not play playlist {}: {}", name, e),
    };
}

/// Stop playback.
fn stop_playback() {
    let status_res = Command::new("/usr/bin/curl")
        .arg("http://127.0.0.1:3000/api/v1/commands/?cmd=stop")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status_res {
        Ok(status) if status.success() => println!("Stopped playback"),
        Ok(status) => eprintln!("Error: Exit status {} when stopping playback", status),
        Err(e) => eprintln!("Error: Could not stop playback: {}", e),
    };
}

/// Shut down the system.
fn shutdown() {
    let status_res = Command::new("/usr/bin/sudo")
        .arg("shutdown")
        .arg("now")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status_res {
        Ok(status) if status.success() => println!("Shutting down"),
        Ok(status) => eprintln!("Error: Exit status {} when shutting down", status),
        Err(e) => eprintln!("Error: Could not shut down: {}", e),
    };
}

/// GPIO input pins.
struct GpioPins {
    aus: InputPin,
    tonabn: InputPin,
    ukw: InputPin,
    kurz: InputPin,
    mittel: InputPin,
    lang: InputPin,
}

type Repetitions = Repeat16;

/// A debouncer for every input pin.
struct Measurements {
    aus: DebouncerStateful<u16, Repetitions>,
    tonabn: DebouncerStateful<u16, Repetitions>,
    ukw: DebouncerStateful<u16, Repetitions>,
    kurz: DebouncerStateful<u16, Repetitions>,
    mittel: DebouncerStateful<u16, Repetitions>,
    lang: DebouncerStateful<u16, Repetitions>,
}

struct GpioPinState {
    pins: GpioPins,
    measurements: Measurements,
}

#[derive(Debug, PartialEq, Eq)]
enum Button {
    Aus,
    Tonabnehmer,
    Ukw,
    Kurz,
    Mittel,
    Lang,
}

impl GpioPinState {
    fn new(pins: GpioPins) -> Self {
        Self {
            pins,
            measurements: Measurements {
                aus: debounce_stateful_16(false),
                tonabn: debounce_stateful_16(false),
                ukw: debounce_stateful_16(false),
                kurz: debounce_stateful_16(false),
                mittel: debounce_stateful_16(false),
                lang: debounce_stateful_16(false),
            },
        }
    }

    /// Update state by reading all inputs.
    fn update(&mut self) -> (Vec<Button>, Vec<Button>) {
        let mut pressed = vec![];
        let mut released = vec![];

        macro_rules! process_pin {
            ($pin:expr, $measurement:expr, $button:expr, $inverted:expr) => {
                match $measurement.update($pin.read() == Level::Low) {
                    Some(Edge::Rising) => if $inverted { released.push($button) } else { pressed.push($button) },
                    Some(Edge::Falling) => if $inverted { pressed.push($button) } else { released.push($button) },
                    None => {}
                }
            };
        }

        process_pin!(self.pins.aus, self.measurements.aus, Button::Aus, true);
        process_pin!(self.pins.tonabn, self.measurements.tonabn, Button::Tonabnehmer, false);
        process_pin!(self.pins.ukw, self.measurements.ukw, Button::Ukw, false);
        process_pin!(self.pins.kurz, self.measurements.kurz, Button::Kurz, false);
        process_pin!(self.pins.mittel, self.measurements.mittel, Button::Mittel, false);
        process_pin!(self.pins.lang, self.measurements.lang, Button::Lang, false);

        (pressed, released)
    }
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

fn gpio_loop(pins: GpioPins, opts: Opts) -> ! {
    let mut state = GpioPinState::new(pins);
    loop {
        // Update measurements
        let (pressed, released) = state.update();

        if !pressed.is_empty() {
            println!("Pressed: {:?}", pressed);

            match pressed[0] {
                Button::Aus => shutdown(),
                Button::Tonabnehmer => play_playlist("jazz"),
                Button::Ukw => play_playlist("mellow"),
                Button::Kurz => play_playlist("world"),
                Button::Mittel => play_playlist("rockblues"),
                Button::Lang => play_playlist("progrock"),
            }
        }
        if !released.is_empty() {
            println!("Released: {:?}", released);
            if pressed.is_empty() {
                stop_playback();
            }
        }

        // Sleep for 10 milliseconds.
        // The debounce count is 16, that means that
        // a signal must be stable for 160ms to trigger
        // the interrupt.
        thread::sleep(Duration::from_millis(10));
    }
}

fn main() {
    let opts: Opts = Opts::parse();

    // Initialize ADC
    let dev = I2cdev::new(&opts.i2c).unwrap();
    let address = SlaveAddr::default();
    let mut adc = Ads1x1x::new_ads1115(dev, address);

    // Initialize GPIO
    let gpio = Gpio::new().expect("Could not initialize GPIO");
    let gpio_pins = GpioPins {
        aus: gpio
            .get(17)
            .expect("Could not init GPIO pin 17")
            .into_input_pullup(),
        tonabn: gpio
            .get(27)
            .expect("Could not init GPIO pin 27")
            .into_input_pullup(),
        ukw: gpio
            .get(22)
            .expect("Could not init GPIO pin 22")
            .into_input_pullup(),
        kurz: gpio
            .get(5)
            .expect("Could not init GPIO pin 5")
            .into_input_pullup(),
        mittel: gpio
            .get(6)
            .expect("Could not init GPIO pin 6")
            .into_input_pullup(),
        lang: gpio
            .get(13)
            .expect("Could not init GPIO pin 13")
            .into_input_pullup(),
    };

    // Configure PGA (gain)
    if let Err(e) = adc.set_full_scale_range(FullScaleRange::Within4_096V) {
        eprintln!("Could not set full scale range: {:?}", e);
        exit(1);
    }

    // Configure sample rate
    if let Err(e) = adc.set_data_rate(DataRate16Bit::Sps32) {
        eprintln!("Warning: Could not set data rate: {:?}", e);
    }

    // Wait for volumio
    wait_for_volumio(&opts.volumio_command);

    // Start threads
    let opts_clone = opts.clone();
    let adc_thread = thread::spawn(move || adc_loop(adc, opts_clone));
    let gpio_thread = thread::spawn(move || gpio_loop(gpio_pins, opts));
    adc_thread.join().unwrap();
    gpio_thread.join().unwrap();
}
