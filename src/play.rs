use std::time::Duration;

pub(crate) mod defaults
{
    pub const VOICES: usize = 32;
    pub const STREAMS: usize = 4;
    pub const STREAM_CHUNK: usize = 65536;
    pub const STREAM_CHUNKS: usize = 3;
    pub const MAX_GAIN: f32 = 1.0;
    pub const MIN_PITCH: f32 = 0.25;
    pub const MAX_PITCH: f32 = 4.0;
    pub const RATE: u32 = 48000;
}

#[derive(Clone, Copy, Debug)]
pub struct Settings
{
    pub m_voices: usize,
    pub m_streams: usize,
    pub m_streamChunk: usize,
    pub m_streamChunks: usize,
    pub m_maxGain: f32,
    pub m_minPitch: f32,
    pub m_maxPitch: f32,
}

impl Settings
{
    pub fn new() -> Settings
    {
        Settings {
            m_voices: defaults::VOICES,
            m_streams: defaults::STREAMS,
            m_streamChunk: defaults::STREAM_CHUNK,
            m_streamChunks: defaults::STREAM_CHUNKS,
            m_maxGain: defaults::MAX_GAIN,
            m_minPitch: defaults::MIN_PITCH,
            m_maxPitch: defaults::MAX_PITCH,
        }
    }

    pub(crate) fn volume(&self, volume: f32) -> f32 { volume.clamp(0.0, self.m_maxGain) }

    pub(crate) fn pitch(&self, pitch: f32) -> f32 { pitch.clamp(self.m_minPitch, self.m_maxPitch) }
}

impl Default for Settings
{
    fn default() -> Self { Self::new() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bus(pub(crate) usize);

impl Bus
{
    pub const MAIN: Bus = Bus(0);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Attenuation
{
    None,
    Linear
    {
        range: f32,
    },
}

impl Attenuation
{
    pub(crate) fn gain(self, distance: f32) -> f32
    {
        match self
        {
            Attenuation::Linear { range } if distance > 0.0 =>
            {
                if distance >= range
                {
                    0.0
                }
                else
                {
                    1.0 - distance / range
                }
            }
            _ => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Voice
{
    pub(crate) m_slot: usize,
    pub(crate) m_id: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Play
{
    pub m_bus: Bus,
    pub m_volume: f32,
    pub m_pitch: f32,
    pub m_looping: bool,
    // a priority voice is only taken over by another voice once every other slot is busy
    pub m_priority: bool,
    // positioned voices pan around the listener and fade with distance
    pub m_position: Option<[f32; 3]>,
    pub m_attenuation: Attenuation,
    pub m_fadeIn: Duration,
    pub m_startAt: f32,
    // reuse the slot of this voice when no other voice took it since
    pub m_replace: Option<Voice>,
}

impl Play
{
    pub fn new() -> Play
    {
        Play {
            m_bus: Bus::MAIN,
            m_volume: 1.0,
            m_pitch: 1.0,
            m_looping: false,
            m_priority: false,
            m_position: None,
            m_attenuation: Attenuation::None,
            m_fadeIn: Duration::ZERO,
            m_startAt: 0.0,
            m_replace: None,
        }
    }
}

impl Default for Play
{
    fn default() -> Self { Self::new() }
}
