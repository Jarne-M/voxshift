//! Opening cpal streams. The callbacks only touch ring buffers and atomics.

use super::block::{BlockAdapter, BlockSource};
use super::shared::{PeakMeter, Shared};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{
    BufferSize, Device, ErrorKind, FromSample, Sample, SampleFormat, SizedSample, Stream,
    StreamConfig, SupportedBufferSize, SupportedStreamConfig,
};
use rtrb::{Consumer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

/// Lets the device manager see, without locks, whether a stream is alive.
#[derive(Debug, Default)]
pub struct StreamHealth {
    failed: AtomicBool,
    heartbeat: AtomicU32,
}

impl StreamHealth {
    #[inline]
    pub fn beat(&self) {
        self.heartbeat.fetch_add(1, Relaxed);
    }

    pub fn heartbeat(&self) -> u32 {
        self.heartbeat.load(Relaxed)
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Relaxed)
    }

    fn report(&self, err: &cpal::Error) {
        if is_fatal(err.kind()) {
            self.failed.store(true, Relaxed);
        }
    }
}

/// Errors after which the stream is dead and must be rebuilt. Glitches (xruns) and the default
/// device moving are not.
pub fn is_fatal(kind: ErrorKind) -> bool {
    !matches!(
        kind,
        ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied
    )
}

/// A running stream plus what we learned about it.
pub struct Opened {
    pub stream: Stream,
    pub rate: u32,
    pub channels: u16,
    /// Frames per device callback (best estimate).
    pub period: u32,
    pub health: Arc<StreamHealth>,
}

fn config_for(supported: &SupportedStreamConfig, buffer: BufferSize) -> StreamConfig {
    StreamConfig {
        channels: supported.channels(),
        sample_rate: supported.sample_rate(),
        buffer_size: buffer,
    }
}

fn period_estimate(stream: &Stream, supported: &SupportedStreamConfig, buffer: BufferSize) -> u32 {
    if let Ok(n) = stream.buffer_size() {
        if n > 0 {
            return n;
        }
    }
    match (buffer, supported.buffer_size()) {
        (BufferSize::Fixed(n), _) => n,
        (_, SupportedBufferSize::Range { max, .. }) if *max > 0 && *max < 1 << 16 => *max,
        _ => supported.sample_rate() / 100, // WASAPI shared mode default: 10 ms
    }
}

fn err_string(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Opens a mic. Frames are downmixed to mono and pushed into the returned consumer; the level
/// goes to `shared.mic_level[slot]`.
pub fn open_input(
    device: &Device,
    buffer: BufferSize,
    shared: Arc<Shared>,
    slot: usize,
) -> Result<(Opened, Consumer<f32>), String> {
    let supported = device.default_input_config().map_err(err_string)?;
    let config = config_for(&supported, buffer);
    // Half a second of headroom: the engine normally keeps it nearly empty.
    let (tx, rx) = RingBuffer::new((config.sample_rate as usize / 2).max(4096));
    let health = Arc::new(StreamHealth::default());
    let ctx = InputCtx {
        tx,
        channels: usize::from(config.channels.max(1)),
        meter: PeakMeter::new(config.sample_rate as f32, 0.3),
        shared,
        slot,
        health: health.clone(),
    };
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_input::<f32>(device, config, ctx),
        SampleFormat::I16 => build_input::<i16>(device, config, ctx),
        SampleFormat::U16 => build_input::<u16>(device, config, ctx),
        SampleFormat::I32 => build_input::<i32>(device, config, ctx),
        SampleFormat::F64 => build_input::<f64>(device, config, ctx),
        SampleFormat::U8 => build_input::<u8>(device, config, ctx),
        other => return Err(format!("unsupported sample format {other}")),
    }
    .map_err(err_string)?;
    stream.play().map_err(err_string)?;
    let period = period_estimate(&stream, &supported, buffer);
    Ok((
        Opened {
            stream,
            rate: config.sample_rate,
            channels: config.channels,
            period,
            health,
        },
        rx,
    ))
}

struct InputCtx {
    tx: rtrb::Producer<f32>,
    channels: usize,
    meter: PeakMeter,
    shared: Arc<Shared>,
    slot: usize,
    health: Arc<StreamHealth>,
}

fn build_input<T>(device: &Device, config: StreamConfig, mut ctx: InputCtx) -> Result<Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let health = ctx.health.clone();
    let scale = 1.0 / ctx.channels as f32;
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            ctx.health.beat();
            let ch = ctx.channels;
            let mono = |f: &[T]| f.iter().map(|&s| s.to_sample::<f32>()).sum::<f32>() * scale;
            let frames = data.len() / ch;
            let peak = data
                .chunks_exact(ch)
                .map(|f| mono(f).abs())
                .fold(0.0f32, f32::max);
            let level = ctx.meter.update(peak, frames);
            ctx.shared.mic_level[ctx.slot].set(level);
            // If the engine is not reading (output stalled) the ring fills up and we drop.
            let n = frames.min(ctx.tx.slots());
            if let Ok(chunk) = ctx.tx.write_chunk_uninit(n) {
                chunk.fill_from_iter(data.chunks_exact(ch).map(mono));
            }
        },
        move |err| health.report(&err),
        None,
    )
}

/// Opens an output stream that pulls blocks from `source`.
pub fn open_output<S: BlockSource + 'static>(
    device: &Device,
    buffer: BufferSize,
    source: S,
) -> Result<Opened, String> {
    let supported = device.default_output_config().map_err(err_string)?;
    open_output_with(device, &supported, buffer, source)
}

pub fn open_output_with<S: BlockSource + 'static>(
    device: &Device,
    supported: &SupportedStreamConfig,
    buffer: BufferSize,
    source: S,
) -> Result<Opened, String> {
    let config = config_for(supported, buffer);
    let health = Arc::new(StreamHealth::default());
    let adapter = BlockAdapter::new(source);
    let h = health.clone();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_output::<f32, S>(device, config, adapter, h),
        SampleFormat::I16 => build_output::<i16, S>(device, config, adapter, h),
        SampleFormat::U16 => build_output::<u16, S>(device, config, adapter, h),
        SampleFormat::I32 => build_output::<i32, S>(device, config, adapter, h),
        SampleFormat::F64 => build_output::<f64, S>(device, config, adapter, h),
        SampleFormat::U8 => build_output::<u8, S>(device, config, adapter, h),
        other => return Err(format!("unsupported sample format {other}")),
    }
    .map_err(err_string)?;
    stream.play().map_err(err_string)?;
    let period = period_estimate(&stream, supported, buffer);
    Ok(Opened {
        stream,
        rate: config.sample_rate,
        channels: config.channels,
        period,
        health,
    })
}

fn build_output<T, S>(
    device: &Device,
    config: StreamConfig,
    mut adapter: BlockAdapter<S>,
    health: Arc<StreamHealth>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample + Sample + FromSample<f32>,
    S: BlockSource + 'static,
{
    let channels = usize::from(config.channels);
    let h = health.clone();
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            health.beat();
            adapter.fill(data, channels);
        },
        move |err| h.report(&err),
        None,
    )
}

/// Default config of an output device without opening it (rate and period for planning).
pub fn output_config(device: &Device) -> Result<SupportedStreamConfig, String> {
    device.default_output_config().map_err(err_string)
}
