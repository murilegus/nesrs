//! 2A03 audio processing unit: two pulse channels, triangle, noise and DMC.
//!
//! The APU is clocked once per CPU cycle. Its output is averaged down to the
//! host sample rate and high-pass filtered to remove the DC offset.

/// NTSC CPU clock in Hz; the APU produces one raw sample per CPU cycle.
pub const CPU_CLOCK_HZ: f64 = 1_789_772.727_272_727_3;

#[rustfmt::skip]
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];

const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];

#[rustfmt::skip]
const TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

const NOISE_PERIODS: [u16; 16] =
    [4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068];

const DMC_PERIODS: [u16; 16] =
    [428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54];

#[derive(Default)]
struct LengthCounter {
    enabled: bool,
    halt: bool,
    value: u8,
}

impl LengthCounter {
    fn load(&mut self, index: u8) {
        if self.enabled {
            self.value = LENGTH_TABLE[index as usize & 0x1F];
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.value = 0;
        }
    }

    fn clock(&mut self) {
        if !self.halt && self.value > 0 {
            self.value -= 1;
        }
    }
}

#[derive(Default)]
struct Envelope {
    start: bool,
    looping: bool,
    constant: bool,
    /// Divider period, doubling as the volume in constant mode.
    volume: u8,
    divider: u8,
    decay: u8,
}

impl Envelope {
    fn write(&mut self, value: u8) {
        self.looping = value & 0x20 != 0;
        self.constant = value & 0x10 != 0;
        self.volume = value & 0x0F;
    }

    fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = self.volume;
        } else if self.divider > 0 {
            self.divider -= 1;
        } else {
            self.divider = self.volume;
            if self.decay > 0 {
                self.decay -= 1;
            } else if self.looping {
                self.decay = 15;
            }
        }
    }

    fn output(&self) -> u8 {
        if self.constant { self.volume } else { self.decay }
    }
}

#[derive(Default)]
struct Pulse {
    /// Pulse 1 negates sweep amounts with ones' complement, pulse 2 with two's.
    ones_complement: bool,
    duty: u8,
    step: u8,
    timer: u16,
    period: u16,
    length: LengthCounter,
    envelope: Envelope,
    sweep_enabled: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_divider: u8,
    sweep_reload: bool,
}

impl Pulse {
    fn write(&mut self, reg: u16, value: u8) {
        match reg {
            0 => {
                self.duty = value >> 6;
                self.length.halt = value & 0x20 != 0;
                self.envelope.write(value);
            }
            1 => {
                self.sweep_enabled = value & 0x80 != 0;
                self.sweep_period = (value >> 4) & 7;
                self.sweep_negate = value & 0x08 != 0;
                self.sweep_shift = value & 7;
                self.sweep_reload = true;
            }
            2 => self.period = (self.period & 0x0700) | value as u16,
            _ => {
                self.period = (self.period & 0x00FF) | ((value as u16 & 7) << 8);
                self.length.load(value >> 3);
                self.step = 0;
                self.envelope.start = true;
            }
        }
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            self.step = (self.step + 1) & 7;
        } else {
            self.timer -= 1;
        }
    }

    fn sweep_target(&self) -> u16 {
        let delta = self.period >> self.sweep_shift;
        if self.sweep_negate {
            self.period.saturating_sub(delta + self.ones_complement as u16)
        } else {
            self.period + delta
        }
    }

    fn muted(&self) -> bool {
        self.period < 8 || self.sweep_target() > 0x7FF
    }

    fn clock_sweep(&mut self) {
        if self.sweep_divider == 0 && self.sweep_enabled && self.sweep_shift > 0 && !self.muted() {
            self.period = self.sweep_target();
        }
        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
    }

    fn output(&self) -> u8 {
        let high = DUTY_TABLE[self.duty as usize][self.step as usize] != 0;
        if high && self.length.value > 0 && !self.muted() { self.envelope.output() } else { 0 }
    }
}

#[derive(Default)]
struct Triangle {
    timer: u16,
    period: u16,
    step: u8,
    length: LengthCounter,
    linear: u8,
    linear_reload_value: u8,
    linear_reload: bool,
}

impl Triangle {
    fn write(&mut self, reg: u16, value: u8) {
        match reg {
            0 => {
                self.length.halt = value & 0x80 != 0;
                self.linear_reload_value = value & 0x7F;
            }
            2 => self.period = (self.period & 0x0700) | value as u16,
            3 => {
                self.period = (self.period & 0x00FF) | ((value as u16 & 7) << 8);
                self.length.load(value >> 3);
                self.linear_reload = true;
            }
            _ => {}
        }
    }

    fn clock_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
            return;
        }
        self.timer = self.period;
        // Periods below 2 produce ultrasonic output; freezing the sequencer
        // instead avoids the resulting pops.
        if self.length.value > 0 && self.linear > 0 && self.period >= 2 {
            self.step = (self.step + 1) & 31;
        }
    }

    fn clock_linear(&mut self) {
        if self.linear_reload {
            self.linear = self.linear_reload_value;
        } else if self.linear > 0 {
            self.linear -= 1;
        }
        // The length-halt bit doubles as the linear counter's control flag.
        if !self.length.halt {
            self.linear_reload = false;
        }
    }

    fn output(&self) -> u8 {
        TRIANGLE_TABLE[self.step as usize]
    }
}

struct Noise {
    timer: u16,
    period: u16,
    short_mode: bool,
    shift: u16,
    length: LengthCounter,
    envelope: Envelope,
}

impl Noise {
    fn new() -> Self {
        Noise {
            timer: 0,
            period: NOISE_PERIODS[0],
            short_mode: false,
            shift: 1,
            length: LengthCounter::default(),
            envelope: Envelope::default(),
        }
    }

    fn write(&mut self, reg: u16, value: u8) {
        match reg {
            0 => {
                self.length.halt = value & 0x20 != 0;
                self.envelope.write(value);
            }
            2 => {
                self.short_mode = value & 0x80 != 0;
                self.period = NOISE_PERIODS[value as usize & 0x0F];
            }
            3 => {
                self.length.load(value >> 3);
                self.envelope.start = true;
            }
            _ => {}
        }
    }

    fn clock_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
            return;
        }
        self.timer = self.period - 1;
        let tap = if self.short_mode { 6 } else { 1 };
        let feedback = (self.shift ^ (self.shift >> tap)) & 1;
        self.shift = self.shift >> 1 | feedback << 14;
    }

    fn output(&self) -> u8 {
        if self.shift & 1 == 0 && self.length.value > 0 { self.envelope.output() } else { 0 }
    }
}

struct Dmc {
    irq_enabled: bool,
    looping: bool,
    period: u16,
    timer: u16,
    output: u8,
    sample_addr: u16,
    sample_len: u16,
    addr: u16,
    remaining: u16,
    buffer: Option<u8>,
    shift: u8,
    bits_left: u8,
    silence: bool,
    irq: bool,
}

impl Dmc {
    fn new() -> Self {
        Dmc {
            irq_enabled: false,
            looping: false,
            period: DMC_PERIODS[0],
            timer: 0,
            output: 0,
            sample_addr: 0xC000,
            sample_len: 1,
            addr: 0xC000,
            remaining: 0,
            buffer: None,
            shift: 0,
            bits_left: 8,
            silence: true,
            irq: false,
        }
    }

    fn write(&mut self, reg: u16, value: u8) {
        match reg {
            0 => {
                self.irq_enabled = value & 0x80 != 0;
                self.looping = value & 0x40 != 0;
                self.period = DMC_PERIODS[value as usize & 0x0F];
                if !self.irq_enabled {
                    self.irq = false;
                }
            }
            1 => self.output = value & 0x7F,
            2 => self.sample_addr = 0xC000 | (value as u16) << 6,
            _ => self.sample_len = (value as u16) << 4 | 1,
        }
    }

    fn restart(&mut self) {
        self.addr = self.sample_addr;
        self.remaining = self.sample_len;
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.irq = false;
        if !enabled {
            self.remaining = 0;
        } else if self.remaining == 0 {
            self.restart();
        }
    }

    /// Address of the next sample byte, if the buffer is empty and wants one.
    fn fetch_request(&self) -> Option<u16> {
        (self.buffer.is_none() && self.remaining > 0).then_some(self.addr)
    }

    fn supply(&mut self, byte: u8) {
        self.buffer = Some(byte);
        self.addr = if self.addr == 0xFFFF { 0x8000 } else { self.addr + 1 };
        self.remaining -= 1;
        if self.remaining == 0 {
            if self.looping {
                self.restart();
            } else if self.irq_enabled {
                self.irq = true;
            }
        }
    }

    fn clock_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
            return;
        }
        self.timer = self.period - 1;
        if !self.silence {
            if self.shift & 1 != 0 {
                if self.output <= 125 {
                    self.output += 2;
                }
            } else if self.output >= 2 {
                self.output -= 2;
            }
        }
        self.shift >>= 1;
        self.bits_left -= 1;
        if self.bits_left == 0 {
            self.bits_left = 8;
            match self.buffer.take() {
                Some(byte) => {
                    self.shift = byte;
                    self.silence = false;
                }
                None => self.silence = true,
            }
        }
    }
}

pub struct Apu {
    pulse1: Pulse,
    pulse2: Pulse,
    triangle: Triangle,
    noise: Noise,
    dmc: Dmc,

    cycle: u64,
    frame_cycle: u32,
    five_step: bool,
    irq_inhibit: bool,
    frame_irq: bool,

    pulse_mix: [f32; 31],
    tnd_mix: [f32; 203],

    sample_rate: f64,
    sample_phase: f64,
    sum: f32,
    count: u32,
    highpass_in: f32,
    highpass_out: f32,
    samples: Vec<f32>,
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

impl Apu {
    pub fn new() -> Self {
        // Standard approximations of the non-linear output mixer.
        let mut pulse_mix = [0.0; 31];
        for (n, slot) in pulse_mix.iter_mut().enumerate().skip(1) {
            *slot = 95.52 / (8128.0 / n as f32 + 100.0);
        }
        let mut tnd_mix = [0.0; 203];
        for (n, slot) in tnd_mix.iter_mut().enumerate().skip(1) {
            *slot = 163.67 / (24329.0 / n as f32 + 100.0);
        }
        Apu {
            pulse1: Pulse { ones_complement: true, ..Pulse::default() },
            pulse2: Pulse::default(),
            triangle: Triangle::default(),
            noise: Noise::new(),
            dmc: Dmc::new(),
            cycle: 0,
            frame_cycle: 0,
            five_step: false,
            irq_inhibit: false,
            frame_irq: false,
            pulse_mix,
            tnd_mix,
            sample_rate: 48_000.0,
            sample_phase: 0.0,
            sum: 0.0,
            count: 0,
            highpass_in: 0.0,
            highpass_out: 0.0,
            samples: Vec::new(),
        }
    }

    /// Reset line: silences every channel.
    pub fn reset(&mut self) {
        self.write(0x4015, 0);
        self.frame_irq = false;
        self.frame_cycle = 0;
    }

    /// Sets the rate, in Hz, at which output samples are generated.
    pub fn set_sample_rate(&mut self, hz: u32) {
        self.sample_rate = hz.max(1) as f64;
    }

    /// Moves the samples generated since the last call into `out`.
    pub fn drain_samples(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.samples);
    }

    /// Level of the APU's IRQ output (frame counter or DMC).
    pub fn irq(&self) -> bool {
        self.frame_irq || self.dmc.irq
    }

    /// Address of the sample byte the DMC channel is waiting for, if any.
    pub fn dmc_fetch_request(&self) -> Option<u16> {
        self.dmc.fetch_request()
    }

    /// Hands the DMC channel the byte it asked for.
    pub fn dmc_supply(&mut self, byte: u8) {
        self.dmc.supply(byte);
    }

    /// Reads `$4015`: channel activity and interrupt flags.
    pub fn read_status(&mut self) -> u8 {
        let status = (self.pulse1.length.value > 0) as u8
            | ((self.pulse2.length.value > 0) as u8) << 1
            | ((self.triangle.length.value > 0) as u8) << 2
            | ((self.noise.length.value > 0) as u8) << 3
            | ((self.dmc.remaining > 0) as u8) << 4
            | (self.frame_irq as u8) << 6
            | (self.dmc.irq as u8) << 7;
        self.frame_irq = false;
        status
    }

    /// Writes one of the APU registers (`$4000-$4013`, `$4015`, `$4017`).
    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4000..=0x4003 => self.pulse1.write(addr & 3, value),
            0x4004..=0x4007 => self.pulse2.write(addr & 3, value),
            0x4008..=0x400B => self.triangle.write(addr & 3, value),
            0x400C..=0x400F => self.noise.write(addr & 3, value),
            0x4010..=0x4013 => self.dmc.write(addr & 3, value),
            0x4015 => {
                self.pulse1.length.set_enabled(value & 0x01 != 0);
                self.pulse2.length.set_enabled(value & 0x02 != 0);
                self.triangle.length.set_enabled(value & 0x04 != 0);
                self.noise.length.set_enabled(value & 0x08 != 0);
                self.dmc.set_enabled(value & 0x10 != 0);
            }
            0x4017 => {
                self.five_step = value & 0x80 != 0;
                self.irq_inhibit = value & 0x40 != 0;
                if self.irq_inhibit {
                    self.frame_irq = false;
                }
                self.frame_cycle = 0;
                if self.five_step {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
            }
            _ => {}
        }
    }

    fn clock_quarter_frame(&mut self) {
        self.pulse1.envelope.clock();
        self.pulse2.envelope.clock();
        self.noise.envelope.clock();
        self.triangle.clock_linear();
    }

    fn clock_half_frame(&mut self) {
        self.pulse1.length.clock();
        self.pulse2.length.clock();
        self.triangle.length.clock();
        self.noise.length.clock();
        self.pulse1.clock_sweep();
        self.pulse2.clock_sweep();
    }

    fn clock_frame_counter(&mut self) {
        self.frame_cycle += 1;
        match (self.five_step, self.frame_cycle) {
            (_, 7457) | (_, 22371) => self.clock_quarter_frame(),
            (_, 14913) | (true, 37281) => {
                self.clock_quarter_frame();
                self.clock_half_frame();
            }
            (false, 29829) => {
                self.clock_quarter_frame();
                self.clock_half_frame();
                if !self.irq_inhibit {
                    self.frame_irq = true;
                }
            }
            (false, 29830) | (true, 37282) => self.frame_cycle = 0,
            _ => {}
        }
    }

    /// Advances the APU by one CPU cycle.
    pub fn tick(&mut self) {
        self.clock_frame_counter();
        self.triangle.clock_timer();
        self.noise.clock_timer();
        self.dmc.clock_timer();
        // The pulse timers run at half the CPU rate.
        if self.cycle & 1 == 0 {
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
        }
        self.cycle += 1;

        self.sum += self.mix();
        self.count += 1;
        self.sample_phase += self.sample_rate;
        if self.sample_phase >= CPU_CLOCK_HZ {
            self.sample_phase -= CPU_CLOCK_HZ;
            self.emit_sample();
        }
    }

    fn mix(&self) -> f32 {
        let pulse = self.pulse1.output() + self.pulse2.output();
        let tnd = 3 * self.triangle.output() + 2 * self.noise.output() + self.dmc.output;
        self.pulse_mix[pulse as usize] + self.tnd_mix[tnd as usize]
    }

    fn emit_sample(&mut self) {
        let input = self.sum / self.count as f32;
        self.sum = 0.0;
        self.count = 0;
        // First-order high-pass (about 40 Hz at 48 kHz) to centre the waveform.
        let output = 0.995 * (self.highpass_out + input - self.highpass_in);
        self.highpass_in = input;
        self.highpass_out = output;
        // Stop buffering if nobody is draining, rather than growing without bound.
        if self.samples.len() < self.sample_rate as usize {
            self.samples.push(output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(apu: &mut Apu, cycles: u32) {
        for _ in 0..cycles {
            apu.tick();
        }
    }

    #[test]
    fn length_counters_load_only_when_enabled_and_report_in_status() {
        let mut apu = Apu::new();
        apu.write(0x4003, 0x08); // channel disabled: ignored
        assert_eq!(apu.read_status() & 0x0F, 0);
        apu.write(0x4015, 0x0F);
        apu.write(0x4003, 0x08);
        apu.write(0x400F, 0x08);
        assert_eq!(apu.read_status() & 0x0F, 0x09);
        apu.write(0x4015, 0x00);
        assert_eq!(apu.read_status() & 0x0F, 0);
    }

    #[test]
    fn length_counter_runs_out_unless_halted() {
        let mut apu = Apu::new();
        apu.write(0x4015, 0x03);
        apu.write(0x4000, 0x00);
        apu.write(0x4004, 0x20); // halt
        apu.write(0x4003, 0x18); // length index 3 = 2 half frames
        apu.write(0x4007, 0x18);
        run(&mut apu, 14913);
        assert_eq!(apu.read_status() & 3, 3);
        run(&mut apu, 29829 - 14913);
        assert_eq!(apu.read_status() & 3, 2);
    }

    #[test]
    fn frame_irq_fires_in_four_step_mode_only() {
        let mut apu = Apu::new();
        run(&mut apu, 29828);
        assert!(!apu.irq());
        run(&mut apu, 1);
        assert!(apu.irq());
        assert_eq!(apu.read_status() & 0x40, 0x40);
        assert!(!apu.irq()); // reading the status acknowledges it

        apu.write(0x4017, 0x80);
        run(&mut apu, 40000);
        assert!(!apu.irq());
        apu.write(0x4017, 0x40);
        run(&mut apu, 40000);
        assert!(!apu.irq());
    }

    #[test]
    fn pulse_produces_a_tone_at_the_programmed_frequency() {
        let mut apu = Apu::new();
        apu.set_sample_rate(48_000);
        apu.write(0x4015, 0x01);
        apu.write(0x4000, 0xBF); // 50% duty, constant volume 15, halt
        apu.write(0x4002, 0xFD); // period 253 -> 1789773 / (16 * 254) = 440.4 Hz
        apu.write(0x4003, 0x00);
        run(&mut apu, CPU_CLOCK_HZ as u32);
        let mut samples = Vec::new();
        apu.drain_samples(&mut samples);
        assert!((47_990..=48_010).contains(&samples.len()));
        // Skip the filter's settling time, then count rising zero crossings.
        let tail = &samples[4800..];
        let crossings = tail.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let hz = crossings as f64 / (tail.len() as f64 / 48_000.0);
        assert!((435.0..=446.0).contains(&hz), "measured {hz} Hz");
        assert!(tail.iter().any(|&s| s > 0.05));
    }

    #[test]
    fn pulse_with_a_period_below_eight_is_muted() {
        let mut apu = Apu::new();
        apu.write(0x4015, 0x01);
        apu.write(0x4000, 0xBF);
        apu.write(0x4002, 0x04);
        apu.write(0x4003, 0x00);
        for _ in 0..10_000 {
            apu.tick();
            assert_eq!(apu.pulse1.output(), 0);
        }
        apu.write(0x4002, 0x08);
        let mut heard = false;
        for _ in 0..10_000 {
            apu.tick();
            heard |= apu.pulse1.output() == 15;
        }
        assert!(heard);
    }

    #[test]
    fn sweep_raises_pitch_until_the_channel_mutes() {
        let mut apu = Apu::new();
        apu.write(0x4015, 0x01);
        apu.write(0x4000, 0xBF);
        apu.write(0x4002, 0x00);
        apu.write(0x4003, 0x04); // period $400
        apu.write(0x4001, 0x81); // enabled, period 0, add, shift 1
        assert!(!apu.pulse1.muted());
        run(&mut apu, 14913);
        assert_eq!(apu.pulse1.period, 0x600);
        assert!(apu.pulse1.muted()); // next target $900 overflows
        run(&mut apu, 30000);
        assert_eq!(apu.pulse1.period, 0x600);
    }

    #[test]
    fn dmc_requests_bytes_plays_them_and_raises_irq() {
        let mut apu = Apu::new();
        apu.write(0x4010, 0x8F); // IRQ on, fastest rate
        apu.write(0x4012, 0x01); // $C040
        apu.write(0x4013, 0x00); // 1 byte
        apu.write(0x4011, 0x40);
        apu.write(0x4015, 0x10);
        assert_eq!(apu.dmc_fetch_request(), Some(0xC040));
        apu.dmc_supply(0xFF);
        assert_eq!(apu.dmc_fetch_request(), None);
        assert!(apu.irq());
        assert_eq!(apu.read_status() & 0x90, 0x80);
        // The byte reaches the shifter at the next 8-bit boundary, then raises
        // the output level by 2 per bit.
        run(&mut apu, 54 * 16);
        assert_eq!(apu.dmc.output, 0x40 + 16);
        apu.write(0x4015, 0x00);
        assert!(!apu.irq());
    }
}
