use super::mixer::{self, MIXED_CHANNELS, SharedMixer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};

#[derive(Debug, thiserror::Error)]
pub enum OutputError
{
    #[error("no default audio output device")]
    NoDevice,
    #[error(transparent)]
    Cpal(#[from] cpal::Error),
}

pub fn open(mixer: SharedMixer) -> Result<Stream, OutputError>
{
    let device: Device = cpal::default_host().default_output_device().ok_or(OutputError::NoDevice)?;
    let config: StreamConfig = device.default_output_config()?.config();
    let channels: usize = config.channels as usize;
    let rate: u32 = config.sample_rate;
    let name: String = device
        .description()
        .map_or_else(|error| format!("unnamed device ({error})"), |description| description.name().to_owned());
    log::info!("Sound output on {name} at {rate} Hz with {channels} channels");
    let mut mixed: Vec<f32> = Vec::new();
    let stream: Stream = device.build_output_stream::<f32, _, _>(
        config,
        move |data, _| {
            mixed.resize(data.len() / channels * MIXED_CHANNELS, 0.0);
            mixer::lock(&mixer).mix(&mut mixed, rate);
            for (frame, stereo) in data.chunks_exact_mut(channels).zip(mixed.as_chunks::<MIXED_CHANNELS>().0)
            {
                writeFrame(frame, stereo);
            }
        },
        |error| log::error!("Sound output error: {error}"),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

fn writeFrame(frame: &mut [f32], stereo: &[f32; MIXED_CHANNELS])
{
    match frame
    {
        [mono] => *mono = (stereo[0] + stereo[1]) / 2.0,
        [left, right, rest @ ..] =>
        {
            *left = stereo[0];
            *right = stereo[1];
            rest.fill(0.0);
        }
        [] =>
        {}
    }
}
