use std::{
    process::{exit, Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use ads1x1x::{channel, Ads1x1x, DataRate16Bit, FullScaleRange, SlaveAddr};
use clap::Clap;
use debouncr::{debounce_stateful_16, DebouncerStateful, Edge, Repeat16};
use embedded_hal::adc::OneShot;
use linux_embedded_hal::I2cdev;
use nb::block;
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use rppal::gpio::{Gpio, InputPin, Level};

#[cfg(test)]
mod tests;

#[derive(Clap, Debug, Clone)]
struct Opts {
    #[clap(default_value = "/dev/i2c-1")]
    i2c: String,
    #[clap(long = "volume-debugging")]
    volume_debugging: bool,
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

/// Set the ALSA volume (percent value 0-100).
fn set_volume(volume: u8, volume_debugging: bool) {
    // Clamp volume to 0-100
    let volume = std::cmp::min(volume, 100);

    // Set volume
    let status_res = Command::new("amixer")
        .arg("-M")
        .arg("set")
        .arg("Digital")
        .arg(&format!("{}%", volume))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status_res {
        Ok(status) if status.success() => {
            if volume_debugging {
                println!("Set volume to {}%", volume);
            }
        }
        Ok(status) => eprintln!("Error: Exit status {} when setting volume", status),
        Err(e) => eprintln!("Error: Could not set volume: {}", e),
    };
}

/// Play a playlist through the API.
fn play_url(url: &str) -> Option<Child> {
    let child_res = Command::new("ffplay")
        .arg("-nodisp")
        .arg("-autoexit")
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child_res {
        Ok(child) => {
            println!("Started playback of URL {}", url);
            child
        }
        Err(e) => {
            eprintln!("Error: Failed to start playback of URL {}: {}", url, e);
            return None;
        }
    };

    // Ensure the child process doesn't exit immediately
    thread::sleep(Duration::from_millis(300));
    match child.try_wait() {
        Ok(Some(status)) => eprintln!("Playback process exited with status {:?}", status.code()),
        Ok(None) => {}
        Err(e) => {
            eprintln!("Error while calling child.try_wait: {}", e);
            return None;
        }
    };

    Some(child)
}

/// Shut down the system.
fn shutdown() {
    let status_res = Command::new("/sbin/halt")
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
                    Some(Edge::Rising) => {
                        if $inverted {
                            released.push($button)
                        } else {
                            pressed.push($button)
                        }
                    }
                    Some(Edge::Falling) => {
                        if $inverted {
                            pressed.push($button)
                        } else {
                            released.push($button)
                        }
                    }
                    None => {}
                }
            };
        }

        process_pin!(self.pins.aus, self.measurements.aus, Button::Aus, true);
        process_pin!(
            self.pins.tonabn,
            self.measurements.tonabn,
            Button::Tonabnehmer,
            false
        );
        process_pin!(self.pins.ukw, self.measurements.ukw, Button::Ukw, false);
        process_pin!(self.pins.kurz, self.measurements.kurz, Button::Kurz, false);
        process_pin!(
            self.pins.mittel,
            self.measurements.mittel,
            Button::Mittel,
            false
        );
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

fn adc_loop(mut adc: Adc, volume_debugging: bool) -> ! {
    // Do measurement
    loop {
        // Analog input 0 ("Lautstärke")
        let a0 = block!(adc.read(&mut channel::SingleA0)).unwrap();
        let volume = map_potentiometer_value(a0 as u16);

        // Analog input 1 ("Klangfarbe")
        let a1 = block!(adc.read(&mut channel::SingleA1)).unwrap();

        // Print values
        if volume_debugging {
            println!("a0={} a1={} vol={}", a0, a1, volume);
        }

        // Set volume
        set_volume(volume, volume_debugging);

        // Sleep for some milliseconds
        thread::sleep(Duration::from_millis(250));
    }
}

enum PlaybackCommand {
    /// Play the specified URL.
    PlayUrl(String),
    /// Stop playback.
    Stop,
}

fn gpio_loop(pins: GpioPins, playback_tx: mpsc::Sender<PlaybackCommand>) -> ! {
    let mut state = GpioPinState::new(pins);
    loop {
        // Update measurements
        let (pressed, released) = state.update();

        // Handle released keys
        if !released.is_empty() {
            println!("Released: {:?}", released);
            if pressed.is_empty() {
                playback_tx
                    .send(PlaybackCommand::Stop)
                    .unwrap_or_else(|e| eprintln!("Error: Failed to send stop command: {}", e));
            }
        }

        // Handle pressed keys
        if !pressed.is_empty() {
            println!("Pressed: {:?}", pressed);

            let play = |url: &str| {
                playback_tx
                    .send(PlaybackCommand::PlayUrl(url.into()))
                    .unwrap_or_else(|e| eprintln!("Error: Failed to send playback command: {}", e))
            };

            match pressed[0] {
                Button::Aus => shutdown(),
                Button::Tonabnehmer => play("http://stream.srg-ssr.ch/m/rsj/mp3_128"),
                Button::Ukw => play("http://stream.radioparadise.com/mellow-flac"),
                Button::Kurz => play("http://stream.radioparadise.com/eclectic-flac"),
                Button::Mittel => play("http://stream.radioparadise.com/rock-flac"),
                Button::Lang => play("http://streamingv2.shoutcast.com/100-PROGRESSIVEROCK"),
            }
        }

        // Sleep for 10 milliseconds.
        // The debounce count is 16, that means that
        // a signal must be stable for 160ms to trigger
        // the interrupt.
        thread::sleep(Duration::from_millis(10));
    }
}

fn playback_loop(playback_rx: mpsc::Receiver<PlaybackCommand>) {
    let mut child: Option<Child> = None;

    fn stop(child: &mut Option<Child>) {
        if let Some(ref mut c) = child {
            if let Err(e) = signal::kill(Pid::from_raw(c.id() as i32), Signal::SIGINT) {
                eprintln!("Could not send SIGINT to child process: {}", e);
            }
            if let Err(e) = c.wait() {
                eprintln!("Error while waiting for playback process to end: {}", e);
            }
        }
        *child = None;
    }

    while let Ok(command) = playback_rx.recv() {
        match command {
            PlaybackCommand::PlayUrl(url) => {
                println!("[playback_loop] Play URL {}", url);
                if child.is_some() {
                    stop(&mut child);
                }
                child = play_url(&url);
            }
            PlaybackCommand::Stop => {
                println!("[playback_loop] Stop playback");
                stop(&mut child);
            }
        }
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

    // Start threads
    let (playback_tx, playback_rx) = mpsc::channel();
    let adc_thread = thread::spawn(move || adc_loop(adc, opts.volume_debugging));
    let gpio_thread = thread::spawn(move || gpio_loop(gpio_pins, playback_tx));
    let playback_thread = thread::spawn(move || playback_loop(playback_rx));
    adc_thread.join().unwrap();
    gpio_thread.join().unwrap();
    playback_thread.join().unwrap();
}
