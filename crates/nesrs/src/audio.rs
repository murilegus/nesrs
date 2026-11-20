//! Sound output. Samples are queued by the emulation loop and pulled by the
//! audio device's callback.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// A running audio output stream.
pub struct AudioOutput {
    queue: Arc<Mutex<VecDeque<f32>>>,
    sample_rate: u32,
    #[cfg(feature = "audio")]
    _stream: cpal::Stream,
}

impl AudioOutput {
    /// Output sample rate of the device in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Amount of audio we aim to keep buffered: enough to ride out scheduling
    /// jitter without adding noticeable latency.
    pub fn target_queue(&self) -> usize {
        self.sample_rate as usize * 60 / 1000
    }

    /// Number of samples waiting to be played.
    pub fn queued(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    /// Queues samples for playback, discarding the oldest if far too many pile up.
    pub fn push(&self, samples: &[f32]) {
        let limit = self.target_queue() * 4;
        let mut queue = self.queue.lock().unwrap();
        queue.extend(samples);
        let excess = queue.len().saturating_sub(limit);
        queue.drain(..excess);
    }
}

#[cfg(feature = "audio")]
mod backend {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample};

    const VOLUME: f32 = 0.8;

    impl AudioOutput {
        /// Opens the default output device and starts playing.
        pub fn start() -> Result<Self, String> {
            let device =
                cpal::default_host().default_output_device().ok_or("no audio output device")?;
            let supported = device.default_output_config().map_err(|e| e.to_string())?;
            let queue = Arc::new(Mutex::new(VecDeque::new()));
            let config = supported.config();
            let stream = match supported.sample_format() {
                SampleFormat::F32 => build::<f32>(&device, &config, queue.clone()),
                SampleFormat::I16 => build::<i16>(&device, &config, queue.clone()),
                SampleFormat::U16 => build::<u16>(&device, &config, queue.clone()),
                SampleFormat::I32 => build::<i32>(&device, &config, queue.clone()),
                other => Err(format!("unsupported sample format {other:?}")),
            }?;
            stream.play().map_err(|e| e.to_string())?;
            Ok(AudioOutput { queue, sample_rate: config.sample_rate, _stream: stream })
        }
    }

    fn build<T: SizedSample + FromSample<f32>>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        queue: Arc<Mutex<VecDeque<f32>>>,
    ) -> Result<cpal::Stream, String> {
        let channels = config.channels as usize;
        let mut last = 0.0f32;
        device
            .build_output_stream(
                *config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let mut queue = queue.lock().unwrap();
                    for frame in data.chunks_mut(channels) {
                        // On underrun, fade the last sample out instead of clicking.
                        last = queue.pop_front().unwrap_or(last * 0.98);
                        frame.fill(T::from_sample(last * VOLUME));
                    }
                },
                |error| eprintln!("nesrs: audio error: {error}"),
                None,
            )
            .map_err(|e| e.to_string())
    }
}

#[cfg(not(feature = "audio"))]
impl AudioOutput {
    /// Sound support was not compiled in.
    pub fn start() -> Result<Self, String> {
        Err("this build has no audio support".into())
    }
}
