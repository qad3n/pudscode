use crate::decode::Format;
use crate::math::{self, Ramp, Vector};
use crate::play::{Attenuation, Bus, Play, Settings, Voice, defaults};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub const MIXED_CHANNELS: usize = 2;

pub type SharedMixer = Arc<Mutex<Mixer>>;

pub fn lock(mixer: &Mutex<Mixer>) -> MutexGuard<'_, Mixer> { mixer.lock().unwrap_or_else(PoisonError::into_inner) }

#[derive(Clone, Copy)]
pub enum Pool
{
    Normal,
    Stream,
}

pub enum Samples
{
    Static(Arc<[f32]>),
    Stream
    {
        queue: VecDeque<f32>,
        ended: bool,
    },
}

impl Samples
{
    pub fn stream() -> Samples { Samples::Stream { queue: VecDeque::new(), ended: false } }

    fn len(&self) -> usize
    {
        match self
        {
            Samples::Static(samples) => samples.len(),
            Samples::Stream { queue, .. } => queue.len(),
        }
    }

    fn get(&self, index: usize) -> f32
    {
        match self
        {
            Samples::Static(samples) => samples[index],
            Samples::Stream { queue, .. } => queue[index],
        }
    }

    fn complete(&self) -> bool
    {
        match self
        {
            Samples::Static(_) => true,
            Samples::Stream { ended, .. } => *ended,
        }
    }
}

struct Playback
{
    m_id: u64,
    m_bus: Bus,
    m_volume: f32,
    m_pitch: f32,
    m_looping: bool,
    m_position: Option<Vector>,
    m_attenuation: Attenuation,
    m_format: Format,
    m_samples: Samples,
    m_cursor: f64,
    m_fade: Ramp,
    m_stopping: bool,
}

impl Playback
{
    fn gains(&self, listener: Vector, right: Vector, maxGain: f32) -> [f32; 2]
    {
        let Some(position) = self.m_position
        else
        {
            let gain: f32 = self.m_volume.min(maxGain);
            return [gain, gain];
        };

        let offset: Vector = math::sub(position, listener);
        let distance: f32 = math::length(offset);
        let gain: f32 = (self.m_attenuation.gain(distance) * self.m_volume).min(maxGain);
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

    // envelope is the fade and bus gain at the start and the end of the block
    fn mix(&mut self, out: &mut [f32], gains: [f32; 2], envelope: [f32; 2], outRate: u32) -> bool
    {
        let channels: usize = self.m_format.m_channels;
        let step: f64 = self.m_pitch as f64 * self.m_format.m_rate as f64 / outRate as f64;
        let wraps: bool = self.m_looping && matches!(self.m_samples, Samples::Static(_));
        let frameCount: f32 = (out.len() / MIXED_CHANNELS) as f32;
        let mut playing: bool = true;
        for (position, frame) in out.as_chunks_mut::<MIXED_CHANNELS>().0.iter_mut().enumerate()
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
            let level: f32 = math::lerp(envelope[0], envelope[1], position as f32 / frameCount);
            frame[0] += left * gains[0] * level;
            frame[1] += right * gains[1] * level;
            self.m_cursor += step;
        }
        self.releaseConsumed();
        playing
    }

    fn releaseConsumed(&mut self)
    {
        let channels: usize = self.m_format.m_channels;
        if let Samples::Stream { queue, .. } = &mut self.m_samples
        {
            let consumed: usize = (self.m_cursor as usize).min(queue.len() / channels);
            queue.drain(..consumed * channels);
            self.m_cursor -= consumed as f64;
        }
    }

    fn faded(&self) -> bool { self.m_stopping && self.m_fade.settled() }
}

struct Slot
{
    m_owner: u64,
    m_priority: bool,
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
    m_settings: Settings,
    m_slots: Vec<Slot>,
    m_cursors: [usize; 2],
    m_nextId: u64,
    m_listener: Listener,
    m_buses: Vec<Ramp>,
    m_envelopes: Vec<[f32; 2]>,
    m_rate: u32,
}

impl Mixer
{
    pub fn new(settings: Settings) -> Mixer
    {
        let slots: Vec<Slot> = (0..settings.m_voices + settings.m_streams)
            .map(|_| Slot { m_owner: 0, m_priority: false, m_playback: None })
            .collect();
        Mixer {
            m_settings: settings,
            m_slots: slots,
            m_cursors: [0, 0],
            m_nextId: 0,
            m_listener: Listener { m_position: [0.0, 0.0, 0.0], m_at: [0.0, 0.0, -1.0], m_up: [0.0, 1.0, 0.0] },
            m_buses: vec![Ramp::new(1.0)],
            m_envelopes: Vec::new(),
            m_rate: defaults::RATE,
        }
    }

    pub fn settings(&self) -> Settings { self.m_settings }

    pub fn setRate(&mut self, rate: u32) { self.m_rate = rate; }

    pub fn start(&mut self, play: &Play, format: Format, samples: Samples, pool: Pool) -> Option<Voice>
    {
        let slot: usize = self.allocate(pool, play.m_replace)?;
        self.m_nextId += 1;
        let cursor: f64 = match samples
        {
            Samples::Static(_) => f64::from(play.m_startAt.max(0.0)) * f64::from(format.m_rate),
            Samples::Stream { .. } => 0.0,
        };
        let mut fade: Ramp = Ramp::new(1.0);
        if !play.m_fadeIn.is_zero()
        {
            fade = Ramp::new(0.0);
            fade.toward(1.0, self.frames(play.m_fadeIn));
        }

        let playback: Playback = Playback {
            m_id: self.m_nextId,
            m_bus: play.m_bus,
            m_volume: self.m_settings.volume(play.m_volume),
            m_pitch: self.m_settings.pitch(play.m_pitch),
            m_looping: play.m_looping,
            m_position: play.m_position,
            m_attenuation: play.m_attenuation,
            m_format: format,
            m_samples: samples,
            m_cursor: cursor,
            m_fade: fade,
            m_stopping: false,
        };
        self.m_slots[slot] = Slot { m_owner: self.m_nextId, m_priority: play.m_priority, m_playback: Some(playback) };
        Some(Voice { m_slot: slot, m_id: self.m_nextId })
    }

    pub fn playing(&self, voice: Voice) -> bool
    {
        self.m_slots
            .get(voice.m_slot)
            .and_then(|slot| slot.m_playback.as_ref())
            .is_some_and(|playback| playback.m_id == voice.m_id)
    }

    pub fn stop(&mut self, voice: Voice, fade: Duration)
    {
        let frames: f32 = self.frames(fade);
        let Some(playback) = self.playback(voice)
        else
        {
            return;
        };

        playback.m_fade.toward(0.0, frames);
        playback.m_stopping = true;
        if playback.faded()
        {
            self.m_slots[voice.m_slot].m_playback = None;
        }
    }

    pub fn setVolume(&mut self, voice: Voice, volume: f32)
    {
        let volume: f32 = self.m_settings.volume(volume);
        if let Some(playback) = self.playback(voice)
        {
            playback.m_volume = volume;
        }
    }

    pub fn setPitch(&mut self, voice: Voice, pitch: f32)
    {
        let pitch: f32 = self.m_settings.pitch(pitch);
        if let Some(playback) = self.playback(voice)
        {
            playback.m_pitch = pitch;
        }
    }

    pub fn setPosition(&mut self, voice: Voice, position: Vector)
    {
        if let Some(playback) = self.playback(voice)
        {
            playback.m_position = Some(position);
        }
    }

    pub fn setListenerPosition(&mut self, position: Vector) { self.m_listener.m_position = position; }

    pub fn setListenerOrientation(&mut self, at: Vector, up: Vector)
    {
        self.m_listener.m_at = at;
        self.m_listener.m_up = up;
    }

    pub fn addBus(&mut self) -> Bus
    {
        self.m_buses.push(Ramp::new(1.0));
        Bus(self.m_buses.len() - 1)
    }

    pub fn setBusVolume(&mut self, bus: Bus, volume: f32, fade: Duration)
    {
        let frames: f32 = self.frames(fade);
        if let Some(ramp) = self.m_buses.get_mut(bus.0)
        {
            ramp.toward(volume.max(0.0), frames);
        }
    }

    pub fn streamQueued(&mut self, voice: Voice) -> Option<usize>
    {
        self.streamSamples(voice).map(|samples| samples.len())
    }

    pub fn appendStream(&mut self, voice: Voice, decoded: &[f32], complete: bool)
    {
        if let Some(Samples::Stream { queue, ended }) = self.streamSamples(voice)
        {
            queue.extend(decoded);
            *ended = complete;
        }
    }

    pub fn mix(&mut self, out: &mut [f32], outRate: u32)
    {
        out.fill(0.0);
        let frames: usize = out.len() / MIXED_CHANNELS;
        self.advanceBuses(frames);
        let listener: Vector = self.m_listener.m_position;
        let right: Vector = math::normalize(math::cross(self.m_listener.m_at, self.m_listener.m_up));
        let maxGain: f32 = self.m_settings.m_maxGain;
        for slot in &mut self.m_slots
        {
            let Some(playback) = slot.m_playback.as_mut()
            else
            {
                continue;
            };

            let gains: [f32; 2] = playback.gains(listener, right, maxGain);
            let fade: [f32; 2] = playback.m_fade.advance(frames);
            let bus: [f32; 2] = self.m_envelopes.get(playback.m_bus.0).copied().unwrap_or([0.0, 0.0]);
            let envelope: [f32; 2] = [fade[0] * bus[0], fade[1] * bus[1]];
            if !playback.mix(out, gains, envelope, outRate) || playback.faded()
            {
                slot.m_playback = None;
            }
        }
        for sample in out.iter_mut()
        {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }

    fn advanceBuses(&mut self, frames: usize)
    {
        self.m_envelopes.clear();
        let main: [f32; 2] = self.m_buses[Bus::MAIN.0].advance(frames);
        self.m_envelopes.push(main);
        for ramp in self.m_buses.iter_mut().skip(1)
        {
            let [start, end] = ramp.advance(frames);
            self.m_envelopes.push([start * main[0], end * main[1]]);
        }
    }

    fn frames(&self, fade: Duration) -> f32 { fade.as_secs_f32() * self.m_rate as f32 }

    fn playback(&mut self, voice: Voice) -> Option<&mut Playback>
    {
        let playback: &mut Playback = self.m_slots.get_mut(voice.m_slot)?.m_playback.as_mut()?;
        (playback.m_id == voice.m_id).then_some(playback)
    }

    fn streamSamples(&mut self, voice: Voice) -> Option<&mut Samples>
    {
        self.playback(voice).map(|playback| &mut playback.m_samples)
    }

    fn pool(&self, pool: Pool) -> (usize, usize, usize)
    {
        match pool
        {
            Pool::Normal => (0, self.m_settings.m_voices, 0),
            Pool::Stream => (self.m_settings.m_voices, self.m_settings.m_streams, 1),
        }
    }

    // round robin over free slots then over slots without priority like paulscode getNextChannel
    fn allocate(&mut self, pool: Pool, replace: Option<Voice>) -> Option<usize>
    {
        let (first, count, cursor): (usize, usize, usize) = self.pool(pool);
        if let Some(voice) = replace
            && (first..first + count).contains(&voice.m_slot)
            && self.m_slots[voice.m_slot].m_owner == voice.m_id
        {
            return Some(voice.m_slot);
        }

        let next: usize = self.m_cursors[cursor];
        for evict in [false, true]
        {
            for step in 0..count
            {
                let slot: usize = (next + step) % count;
                if !self.free(first + slot, evict)
                {
                    continue;
                }

                self.m_cursors[cursor] = (slot + 1) % count;
                return Some(first + slot);
            }
        }
        None
    }

    fn free(&self, slot: usize, evict: bool) -> bool
    {
        let slot: &Slot = &self.m_slots[slot];
        slot.m_playback.is_none() || evict && !slot.m_priority
    }
}
