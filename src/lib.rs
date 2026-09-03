use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    Device, StreamConfig, SupportedStreamConfig,
};

/// Shared audio pipeline between the mic (producer) and the speaker (consumer).
///
/// The microphone and the speaker are independent hardware with their own
/// sample rates and channel counts. To bridge them we reduce everything to a
/// mono stream: mic frames are down-mixed to mono and stored in a ring; the
/// speaker callback pulls one mono sample per output frame (resampling across
/// the two rates when needed) and up-mixes it to every output channel.
///
/// A playback cushion (`lag`) keeps the consumer reading data that is already
/// buffered instead of racing the live edge, and the ring is capped so a
/// faster producer cannot make latency (or memory) grow without bound.
pub struct AudioBus {
    out_channels: usize,
    /// Input samples to consume per output sample (input_rate / output_rate).
    ratio: f64,
    /// Minimum amount of mono input to keep buffered ahead of the play head,
    /// expressed in input samples (~50 ms).
    lag: f64,
    /// Above this cushion we discard the surplus to bound total latency.
    max_lag: f64,
    ring: VecDeque<f32>,
    /// Absolute sample index of the oldest sample in `ring`.
    consumed: u64,
    /// Fractional read position in the mono input timeline.
    pos: f64,
}

impl AudioBus {
    pub fn new(input: &SupportedStreamConfig, output: &SupportedStreamConfig) -> AudioBus {
        AudioBus::init(
            input.sample_rate().0,
            output.sample_rate().0,
            output.channels() as usize,
        )
    }

    fn init(in_rate: u32, out_rate: u32, out_channels: usize) -> AudioBus {
        let lag = (in_rate as f64 * 0.05).round().max(128.0);
        AudioBus {
            out_channels,
            ratio: in_rate as f64 / out_rate as f64,
            lag,
            max_lag: (in_rate as f64 * 0.25).round().max(lag * 2.0),
            ring: VecDeque::with_capacity((in_rate as f64 * 0.1) as usize),
            consumed: 0,
            pos: 0.0,
        }
    }

    /// Down-mix one capture chunk to mono and append it to the ring.
    fn push_frames(&mut self, data: &[f32], channels: usize) {
        if channels == 0 {
            return;
        }
        for frame in data.chunks_exact(channels) {
            // If the consumer is far behind, drop the oldest audio instead of
            // letting the delay (or the buffer) grow without end.
            if self.ring.len() as f64 >= self.max_lag {
                self.ring.pop_front();
                self.consumed += 1;
            }
            let mono = frame.iter().sum::<f32>() / channels as f32;
            self.ring.push_back(mono);
        }
    }

    fn sample_at(&self, index: u64) -> f32 {
        if index < self.consumed {
            return 0.0;
        }
        let local = (index - self.consumed) as usize;
        match self.ring.get(local) {
            Some(&s) => s,
            None => 0.0, // not produced yet -> silence
        }
    }

    /// Fill the whole output buffer from the mono ring.
    fn fill_output(&mut self, data: &mut [f32]) {
        if self.out_channels == 0 {
            return;
        }

        // The producer may have dropped old samples we had not reached yet.
        if (self.pos as u64) < self.consumed {
            self.pos = self.consumed as f64;
        }

        // A producer that runs much faster than the consumer would let the
        // play head fall far behind; skip the surplus to keep latency small.
        let produced = self.consumed as f64 + self.ring.len() as f64;
        if produced - self.pos > self.max_lag {
            self.pos = produced - self.lag;
        }

        let frames = data.len() / self.out_channels;
        for f in 0..frames {
            let produced = self.consumed as f64 + self.ring.len() as f64;
            let base = f * self.out_channels;

            // Keep a cushion of data buffered so we never run into the edge of
            // what the producer has delivered yet (startup / real underruns).
            if produced - self.pos < self.lag {
                for c in 0..self.out_channels {
                    data[base + c] = 0.0;
                }
                continue;
            }

            let p = self.pos;
            let i = p.floor() as u64;
            let frac = (p - i as f64) as f32;
            let s0 = self.sample_at(i);
            let s1 = self.sample_at(i + 1);
            let mono = s0 + (s1 - s0) * frac;

            for c in 0..self.out_channels {
                data[base + c] = mono;
            }
            self.pos += self.ratio;
        }

        // Forget samples the play head has already passed.
        let floor = self.pos.floor() as u64;
        while !self.ring.is_empty() && self.consumed < floor {
            self.ring.pop_front();
            self.consumed += 1;
        }
    }
}

pub fn mic(bus: Arc<Mutex<AudioBus>>, input_device: Device, input_config: SupportedStreamConfig) {
    thread::spawn(move || {
        let channels = input_config.channels() as usize;
        let config: StreamConfig = input_config.into();
        let input_stream = input_device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut bus) = bus.lock() {
                        bus.push_frames(data, channels);
                    }
                },
                move |err| {
                    eprintln!("输入流错误: {:?}", err);
                },
                None,
            )
            .expect("构建输入流时出错");

        input_stream.play().expect("输入流运行错误");
        loop {
            thread::sleep(Duration::from_secs(60));
        }
    });
}

pub fn loud(bus: Arc<Mutex<AudioBus>>, output_device: Device, output_config: SupportedStreamConfig) {
    thread::spawn(move || {
        let config: StreamConfig = output_config.into();
        let output_stream = output_device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if let Ok(mut bus) = bus.lock() {
                        bus.fill_output(data);
                    }
                },
                move |err| {
                    eprintln!("输出流错误: {:?}", err);
                },
                None,
            )
            .expect("构建输出流时出错");

        output_stream.play().expect("输出流运行错误");
        loop {
            thread::sleep(Duration::from_secs(60));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::AudioBus;

    /// Simulates a constant-signal mic (mono `level`) feeding the bus, then the
    /// speaker pulling `out_frames` stereo frames. Returns whether every channel
    /// ended up at `level` and how big the ring grew.
    fn run_bridge(
        in_rate: u32,
        out_rate: u32,
        in_ch: usize,
        out_ch: usize,
        seconds: f64,
        level: f32,
        out_frames: usize,
    ) -> (bool, usize) {
        let mut bus = AudioBus::init(in_rate, out_rate, out_ch);

        let in_samples = (in_rate as f64 * seconds).round() as usize;
        let chunk = vec![level; 4096];
        let mut fed = 0;
        while fed < in_samples {
            let n = chunk.len().min(in_samples - fed);
            bus.push_frames(&chunk[..n], in_ch);
            fed += n;
        }

        let mut data = vec![0.0f32; out_frames * out_ch];
        bus.fill_output(&mut data);

        let all_level = data.iter().all(|&s| (s - level).abs() < 1e-3);
        (all_level, bus.ring.len())
    }

    #[test]
    fn equal_rates_mono_in_stereo_out() {
        // Mono mic 48k -> stereo speakers 48k (the most common breakage).
        let (ok, ring) = run_bridge(48_000, 48_000, 1, 2, 1.0, 0.5, 4800);
        assert!(ok, "output was not reproduced across the channel gap");
        assert!(ring as f64 <= 48_000.0 * 0.25 + 2.0, "ring grew unbounded: {}", ring);
    }

    #[test]
    fn differing_rates_are_resampled() {
        // 44.1k mic into 48k speakers must still play at correct level.
        let (ok, _) = run_bridge(44_100, 48_000, 1, 2, 1.0, 0.25, 4800);
        assert!(ok, "cross-rate output was not reproduced");
    }

    #[test]
    fn ring_is_bounded_under_producer_overload() {
        let mut bus = AudioBus::init(48_000, 48_000, 2);
        let chunk = vec![0.5f32; 4096];
        // Push ~10x more than the cap without consuming.
        for _ in 0..120 {
            bus.push_frames(&chunk, 1);
        }
        assert!(
            bus.ring.len() as f64 <= bus.max_lag + 2.0,
            "ring length {} exceeded cap {}",
            bus.ring.len(),
            bus.max_lag
        );
    }

    #[test]
    fn initial_silence_then_audio_after_cushion() {
        // Until the producer has buffered ~lag samples, output must be silence.
        let mut bus = AudioBus::init(48_000, 48_000, 2);
        // Deliver only a tiny amount of input, then ask for a big output block.
        bus.push_frames(&vec![0.5f32; 100], 1);
        let mut data = vec![0.0f32; 4800 * 2];
        bus.fill_output(&mut data);
        assert!(
            data.iter().all(|&s| s == 0.0),
            "expected silence before the cushion is full"
        );
    }
}
