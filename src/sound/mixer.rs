use super::decode::{Format, Pcm};
use super::math::{self, Vector};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub const ATTENUATION_NONE: i32 = 0;
pub const ATTENUATION_LINEAR: i32 = 2;
pub const MIXED_CHANNELS: usize = 2;

const NORMAL_CHANNELS: usize = 28;
const STREAMING_CHANNELS: usize = 4;

mod limits
{
    pub const MAX_GAIN: f32 = 1.0;
    pub const MIN_PITCH: f32 = 0.5;
    pub const MAX_PITCH: f32 = 2.0;
}

pub type SharedMixer = Arc<Mutex<Mixer>>;

pub fn lock(mixer: &Mutex<Mixer>) -> MutexGuard<'_, Mixer> { mixer.lock().unwrap_or_else(PoisonError::into_inner) }

#[derive(Clone, Copy)]
pub struct Params
{
    m_position: Vector,
    m_attModel: i32,
    m_distOrRoll: f32,
    m_loop: bool,
    m_volume: f32,
    m_pitch: f32,
}

impl Params
{
    pub fn new(position: Vector, attModel: i32, distOrRoll: f32, toLoop: bool) -> Params
    {
        Params {
            m_position: position,
            m_attModel: attModel,
            m_distOrRoll: distOrRoll,
            m_loop: toLoop,
            m_volume: 1.0,
            m_pitch: 1.0,
        }
    }

    fn distanceGain(&self, distance: f32) -> f32
    {
        if self.m_attModel != ATTENUATION_LINEAR || distance <= 0.0
        {
            return 1.0;
        }
        if distance >= self.m_distOrRoll
        {
            0.0
        }
        else
        {
            1.0 - distance / self.m_distOrRoll
        }
    }
}

#[derive(Clone)]
pub struct StreamFile
{
    pub m_url: String,
    pub m_identifier: String,
}

pub enum SourceData
{
    Static(Arc<Pcm>),
    Stream(StreamFile),
}

struct Source
{
    m_id: u64,
    m_priority: bool,
    m_params: Params,
    m_data: SourceData,
    m_voice: Option<usize>,
}

enum Samples
{
    Static(Arc<[f32]>),
    Stream
    {
        m_queue: VecDeque<f32>,
        m_ended: bool,
    },
}

impl Samples
{
    fn len(&self) -> usize
    {
        match self
        {
            Samples::Static(samples) => samples.len(),
            Samples::Stream { m_queue, .. } => m_queue.len(),
        }
    }

    fn get(&self, index: usize) -> f32
    {
        match self
        {
            Samples::Static(samples) => samples[index],
            Samples::Stream { m_queue, .. } => m_queue[index],
        }
    }

    fn complete(&self) -> bool
    {
        match self
        {
            Samples::Static(_) => true,
            Samples::Stream { m_ended, .. } => *m_ended,
        }
    }
}

struct Playback
{
    m_sourceId: u64,
    m_params: Params,
    m_format: Format,
    m_samples: Samples,
    m_cursor: f64,
}

impl Playback
{
    fn gains(&self, listener: Vector, right: Vector) -> [f32; 2]
    {
        let offset: Vector = math::sub(self.m_params.m_position, listener);
        let distance: f32 = math::length(offset);
        let gain: f32 = (self.m_params.distanceGain(distance) * self.m_params.m_volume).min(limits::MAX_GAIN);
        if self.m_format.m_channels > 1
        {
            return [gain, gain];
        }
        let pan: f32 = if distance > 0.0
        {
            math::dot(offset, right) / distance
        }
        else
        {
            0.0
        };
        let [left, right] = math::equalPowerPan(pan);
        [gain * left, gain * right]
    }

    fn mix(&mut self, out: &mut [f32], gains: [f32; 2], outRate: u32) -> bool
    {
        let channels: usize = self.m_format.m_channels;
        let step: f64 = self.m_params.m_pitch as f64 * self.m_format.m_rate as f64 / outRate as f64;
        let wraps: bool = self.m_params.m_loop && matches!(self.m_samples, Samples::Static(_));
        let mut playing: bool = true;
        for frame in out.as_chunks_mut::<MIXED_CHANNELS>().0
        {
            let frames: usize = self.m_samples.len() / channels;
            let complete: bool = self.m_samples.complete();
            if self.m_cursor as usize >= frames && complete
            {
                if !wraps || frames == 0
                {
                    playing = false;
                    break;
                }
                self.m_cursor %= frames as f64;
            }
            let index: usize = self.m_cursor as usize;
            if index + 1 >= frames && !complete
            {
                break;
            }
            let next: usize = if index + 1 < frames
            {
                index + 1
            }
            else if wraps
            {
                0
            }
            else
            {
                index
            };
            let fraction: f32 = (self.m_cursor - index as f64) as f32;
            let sample = |channel: usize| {
                math::lerp(
                    self.m_samples.get(index * channels + channel),
                    self.m_samples.get(next * channels + channel),
                    fraction,
                )
            };
            let left: f32 = sample(0);
            let right: f32 = if channels == 1 { left } else { sample(1) };
            frame[0] += left * gains[0];
            frame[1] += right * gains[1];
            self.m_cursor += step;
        }
        self.releaseConsumed();
        playing
    }

    fn releaseConsumed(&mut self)
    {
        let channels: usize = self.m_format.m_channels;
        if let Samples::Stream { m_queue, .. } = &mut self.m_samples
        {
            let consumed: usize = (self.m_cursor as usize).min(m_queue.len() / channels);
            m_queue.drain(..consumed * channels);
            self.m_cursor -= consumed as f64;
        }
    }
}

struct Voice
{
    m_sourceName: Option<String>,
    m_playback: Option<Playback>,
}

struct Listener
{
    m_position: Vector,
    m_at: Vector,
    m_up: Vector,
}

pub struct Mixer
{
    m_sources: HashMap<String, Source>,
    m_voices: Vec<Voice>,
    m_nextNormal: usize,
    m_nextStreaming: usize,
    m_nextId: u64,
    m_listener: Listener,
}

impl Mixer
{
    pub fn new() -> Mixer
    {
        let voices: Vec<Voice> =
            (0..NORMAL_CHANNELS + STREAMING_CHANNELS).map(|_| Voice { m_sourceName: None, m_playback: None }).collect();
        Mixer {
            m_sources: HashMap::new(),
            m_voices: voices,
            m_nextNormal: 0,
            m_nextStreaming: 0,
            m_nextId: 0,
            m_listener: Listener { m_position: [0.0, 0.0, 0.0], m_at: [0.0, 0.0, -1.0], m_up: [0.0, 1.0, 0.0] },
        }
    }

    pub fn newSource(&mut self, name: String, priority: bool, params: Params, data: SourceData)
    {
        self.m_nextId += 1;
        let source: Source =
            Source { m_id: self.m_nextId, m_priority: priority, m_params: params, m_data: data, m_voice: None };
        self.m_sources.insert(name, source);
    }

    pub fn isPlaying(&self, name: &str) -> bool { self.attachedVoice(name).is_some() }

    pub fn streamToOpen(&self, name: &str) -> Option<(StreamFile, bool)>
    {
        let source: &Source = self.m_sources.get(name)?;
        match &source.m_data
        {
            SourceData::Stream(file) if !self.isPlaying(name) => Some((file.clone(), source.m_params.m_loop)),
            _ => None,
        }
    }

    pub fn play(&mut self, name: &str, streamFormat: Option<Format>) -> Option<(usize, u64)>
    {
        if self.isPlaying(name)
        {
            return None;
        }
        let source: &Source = self.m_sources.get(name)?;
        let (format, samples): (Format, Samples) = match (&source.m_data, streamFormat)
        {
            (SourceData::Static(pcm), _) => (pcm.m_format, Samples::Static(pcm.m_samples.clone())),
            (SourceData::Stream(_), Some(format)) =>
            {
                (format, Samples::Stream { m_queue: VecDeque::new(), m_ended: false })
            }
            (SourceData::Stream(_), None) => return None,
        };
        let sourceId: u64 = source.m_id;
        let playback: Playback = Playback {
            m_sourceId: sourceId,
            m_params: source.m_params,
            m_format: format,
            m_samples: samples,
            m_cursor: 0.0,
        };
        let stream: bool = matches!(source.m_data, SourceData::Stream(_));
        let voice: usize = self.nextChannel(name, stream)?;
        self.m_voices[voice].m_playback = Some(playback);
        if let Some(source) = self.m_sources.get_mut(name)
        {
            source.m_voice = Some(voice);
        }
        Some((voice, sourceId))
    }

    pub fn stop(&mut self, name: &str)
    {
        if let Some(voice) = self.attachedVoice(name)
        {
            self.m_voices[voice].m_playback = None;
        }
    }

    pub fn setVolume(&mut self, name: &str, volume: f32)
    {
        self.updateParams(name, |params| params.m_volume = volume.clamp(0.0, limits::MAX_GAIN));
    }

    pub fn setPitch(&mut self, name: &str, pitch: f32)
    {
        self.updateParams(name, |params| params.m_pitch = pitch.clamp(limits::MIN_PITCH, limits::MAX_PITCH));
    }

    pub fn setListenerPosition(&mut self, position: Vector) { self.m_listener.m_position = position; }

    pub fn setListenerOrientation(&mut self, at: Vector, up: Vector)
    {
        self.m_listener.m_at = at;
        self.m_listener.m_up = up;
    }

    pub fn streamQueued(&mut self, voice: usize, sourceId: u64) -> Option<usize>
    {
        self.streamSamples(voice, sourceId).map(|samples| samples.len())
    }

    pub fn appendStream(&mut self, voice: usize, sourceId: u64, decoded: &[f32], ended: bool)
    {
        if let Some(Samples::Stream { m_queue, m_ended }) = self.streamSamples(voice, sourceId)
        {
            m_queue.extend(decoded);
            *m_ended = ended;
        }
    }

    pub fn mix(&mut self, out: &mut [f32], outRate: u32)
    {
        out.fill(0.0);
        let listener: Vector = self.m_listener.m_position;
        let right: Vector = math::normalize(math::cross(self.m_listener.m_at, self.m_listener.m_up));
        for voice in &mut self.m_voices
        {
            let Some(playback) = voice.m_playback.as_mut()
            else
            {
                continue;
            };
            let gains: [f32; 2] = playback.gains(listener, right);
            if !playback.mix(out, gains, outRate)
            {
                voice.m_playback = None;
            }
        }
        for sample in out.iter_mut()
        {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }

    fn attachedVoice(&self, name: &str) -> Option<usize>
    {
        let source: &Source = self.m_sources.get(name)?;
        let voice: usize = source.m_voice?;
        let playback: &Playback = self.m_voices[voice].m_playback.as_ref()?;
        (playback.m_sourceId == source.m_id).then_some(voice)
    }

    fn updateParams(&mut self, name: &str, update: impl Fn(&mut Params))
    {
        let voice: Option<usize> = self.attachedVoice(name);
        let Some(source) = self.m_sources.get_mut(name)
        else
        {
            return;
        };
        update(&mut source.m_params);
        if let Some(playback) = voice.and_then(|voice| self.m_voices[voice].m_playback.as_mut())
        {
            update(&mut playback.m_params);
        }
    }

    fn streamSamples(&mut self, voice: usize, sourceId: u64) -> Option<&mut Samples>
    {
        let playback: &mut Playback = self.m_voices[voice].m_playback.as_mut()?;
        (playback.m_sourceId == sourceId).then_some(&mut playback.m_samples)
    }

    fn nextChannel(&mut self, name: &str, stream: bool) -> Option<usize>
    {
        let (first, count, next): (usize, usize, usize) = if stream
        {
            (NORMAL_CHANNELS, STREAMING_CHANNELS, self.m_nextStreaming)
        }
        else
        {
            (0, NORMAL_CHANNELS, self.m_nextNormal)
        };
        if let Some(voice) =
            (first..first + count).find(|&voice| self.m_voices[voice].m_sourceName.as_deref() == Some(name))
        {
            return Some(voice);
        }
        for evictLowPriority in [false, true]
        {
            for step in 0..count
            {
                let slot: usize = (next + step) % count;
                if !self.channelFree(first + slot, evictLowPriority)
                {
                    continue;
                }
                let following: usize = (slot + 1) % count;
                if stream
                {
                    self.m_nextStreaming = following;
                }
                else
                {
                    self.m_nextNormal = following;
                }
                self.m_voices[first + slot].m_sourceName = Some(name.to_owned());
                return Some(first + slot);
            }
        }
        None
    }

    fn channelFree(&self, voice: usize, evictLowPriority: bool) -> bool
    {
        let Some(name) = self.m_voices[voice].m_sourceName.as_deref()
        else
        {
            return true;
        };
        let Some(source) = self.m_sources.get(name)
        else
        {
            return true;
        };
        !self.isPlaying(name) || evictLowPriority && !source.m_priority
    }
}
