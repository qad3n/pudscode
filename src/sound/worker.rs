use super::load;
use crate::decode::{Sound, Source};
use crate::engine::Engine;
use crate::play::{Play, Voice};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub type Attached = Arc<Mutex<HashMap<String, Voice>>>;

pub fn lock(attached: &Attached) -> MutexGuard<'_, HashMap<String, Voice>>
{
    attached.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct SourceSpec
{
    pub m_name: String,
    pub m_stream: bool,
    pub m_url: String,
    pub m_identifier: String,
    pub m_play: Play,
}

pub enum Command
{
    NewSource(SourceSpec),
    Play(String),
    PlayFrom(String, f32),
    Stop(String),
    SetVolume(String, f32),
    SetPitch(String, f32),
    ListenerPosition([f32; 3]),
    ListenerOrientation([f32; 3], [f32; 3]),
}

enum Data
{
    Static(Sound),
    Stream
    {
        url: String,
        identifier: String,
    },
}

struct Named
{
    m_play: Play,
    m_data: Data,
    // the channel this name last played on which a new source of the same name takes over
    m_voice: Option<Voice>,
}

struct Worker
{
    m_engine: Arc<Engine>,
    m_attached: Attached,
    m_sources: HashMap<String, Named>,
    m_buffers: HashMap<String, Option<Sound>>,
}

pub fn run(engine: Arc<Engine>, attached: Attached, commands: Receiver<Command>)
{
    let mut worker: Worker =
        Worker { m_engine: engine, m_attached: attached, m_sources: HashMap::new(), m_buffers: HashMap::new() };
    while let Ok(command) = commands.recv()
    {
        worker.execute(command);
    }
}

impl Worker
{
    fn execute(&mut self, command: Command)
    {
        match command
        {
            Command::NewSource(spec) => self.newSource(spec),
            Command::Play(name) => self.play(&name, 0.0),
            Command::PlayFrom(name, seconds) => self.play(&name, seconds),
            Command::Stop(name) => self.stop(&name),
            Command::SetVolume(name, volume) => self.update(&name, |play| play.m_volume = volume),
            Command::SetPitch(name, pitch) => self.update(&name, |play| play.m_pitch = pitch),
            Command::ListenerPosition(position) => self.m_engine.setListenerPosition(position),
            Command::ListenerOrientation(at, up) => self.m_engine.setListenerOrientation(at, up),
        }
    }

    fn newSource(&mut self, spec: SourceSpec)
    {
        let data: Data = if spec.m_stream
        {
            Data::Stream { url: spec.m_url, identifier: spec.m_identifier }
        }
        else
        {
            let Some(sound) = self.buffer(&spec.m_url, &spec.m_identifier)
            else
            {
                return;
            };
            Data::Static(sound)
        };
        let voice: Option<Voice> = self.m_sources.get(&spec.m_name).and_then(|named| named.m_voice);
        lock(&self.m_attached).remove(&spec.m_name);
        self.m_sources.insert(spec.m_name, Named { m_play: spec.m_play, m_data: data, m_voice: voice });
    }

    fn buffer(&mut self, url: &str, identifier: &str) -> Option<Sound>
    {
        let decode = || match load::sound(url, identifier)
        {
            Ok(sound) => Some(sound),
            Err(error) =>
            {
                log::error!("Error loading {identifier}: {error}");
                None
            }
        };
        self.m_buffers.entry(identifier.to_owned()).or_insert_with(decode).clone()
    }

    fn play(&mut self, name: &str, start: f32)
    {
        if self.attached(name).is_some()
        {
            return;
        }

        let Some(named) = self.m_sources.get(name)
        else
        {
            return;
        };

        let play: Play = Play { m_replace: named.m_voice, ..named.m_play };
        let voice: Option<Voice> = match &named.m_data
        {
            Data::Static(sound) => self.m_engine.play(sound, play),
            Data::Stream { url, identifier } => self.stream(name, url, identifier, Play { m_startAt: start, ..play }),
        };
        let Some(voice) = voice
        else
        {
            return;
        };

        if let Some(named) = self.m_sources.get_mut(name)
        {
            named.m_voice = Some(voice);
        }
        lock(&self.m_attached).insert(name.to_owned(), voice);
    }

    fn stream(&self, name: &str, url: &str, identifier: &str, play: Play) -> Option<Voice>
    {
        let started: Result<Option<Voice>, load::LoadError> =
            load::source(url, identifier).and_then(|source: Source| Ok(self.m_engine.stream(&source, play)?));
        match started
        {
            Ok(voice) =>
            {
                log::debug!("Streaming {identifier} on {name}");
                voice
            }
            Err(error) =>
            {
                log::error!("Error opening stream {identifier}: {error}");
                None
            }
        }
    }

    fn stop(&self, name: &str)
    {
        if let Some(voice) = self.attached(name)
        {
            self.m_engine.stop(voice, Duration::ZERO);
        }
    }

    // paulscode keeps the value on the source so it holds for the next play too
    fn update(&mut self, name: &str, change: impl Fn(&mut Play))
    {
        let voice: Option<Voice> = self.attached(name);
        let Some(named) = self.m_sources.get_mut(name)
        else
        {
            return;
        };

        change(&mut named.m_play);
        if let Some(voice) = voice
        {
            self.m_engine.setVolume(voice, named.m_play.m_volume);
            self.m_engine.setPitch(voice, named.m_play.m_pitch);
        }
    }

    fn attached(&self, name: &str) -> Option<Voice>
    {
        let voice: Voice = lock(&self.m_attached).get(name).copied()?;
        self.m_engine.playing(voice).then_some(voice)
    }
}
