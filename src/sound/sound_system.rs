use super::worker::{self, Attached, Command, SourceSpec};
use crate::engine::Engine;
use crate::play::{Attenuation, Bus, Play, Settings, Voice};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const ATTENUATION_NONE: i32 = 0;
const ATTENUATION_LINEAR: i32 = 2;

// channel counts stream buffers and clamps of LibraryLWJGLOpenAL
fn settings() -> Settings
{
    Settings {
        m_voices: 28,
        m_streams: 4,
        m_streamChunk: 65536,
        m_streamChunks: 3,
        m_maxGain: 1.0,
        m_minPitch: 0.5,
        m_maxPitch: 2.0,
    }
}

fn place(priority: bool, toLoop: bool, position: [f32; 3], attModel: i32, distOrRoll: f32) -> Play
{
    let attenuation: Attenuation = if attModel == ATTENUATION_LINEAR
    {
        Attenuation::Linear { range: distOrRoll }
    }
    else
    {
        Attenuation::None
    };
    Play {
        m_bus: Bus::MAIN,
        m_volume: 1.0,
        m_pitch: 1.0,
        m_looping: toLoop,
        m_priority: priority,
        m_position: Some(position),
        m_attenuation: attenuation,
        m_fadeIn: Duration::ZERO,
        m_startAt: 0.0,
        m_replace: None,
    }
}

struct Backend
{
    m_engine: Arc<Engine>,
    m_attached: Attached,
    m_commands: Sender<Command>,
    m_worker: JoinHandle<()>,
}

impl Backend
{
    fn start() -> Option<Backend>
    {
        let engine: Arc<Engine> = Arc::new(Engine::new(settings()));
        if !engine.audible()
        {
            return None;
        }

        let attached: Attached = Arc::new(Mutex::new(HashMap::new()));
        let (commands, receiver): (Sender<Command>, Receiver<Command>) = mpsc::channel();
        let workerEngine: Arc<Engine> = engine.clone();
        let workerAttached: Attached = attached.clone();
        let spawned: std::io::Result<JoinHandle<()>> = std::thread::Builder::new()
            .name("Sound Command Thread".to_owned())
            .spawn(move || worker::run(workerEngine, workerAttached, receiver));
        match spawned
        {
            Ok(worker) =>
            {
                Some(Backend { m_engine: engine, m_attached: attached, m_commands: commands, m_worker: worker })
            }
            Err(error) =>
            {
                log::error!("Could not start the sound thread, sound stays silent: {error}");
                None
            }
        }
    }

    fn stop(self)
    {
        let Backend { m_engine, m_attached: _, m_commands, m_worker } = self;
        drop(m_commands);
        if m_worker.join().is_err()
        {
            log::error!("Sound thread panicked during cleanup");
        }
        // the worker held the other reference so this drop closes the device
        drop(m_engine);
    }
}

pub struct SoundSystem
{
    m_backend: RefCell<Option<Backend>>,
}

impl SoundSystem
{
    pub fn new() -> SoundSystem { SoundSystem { m_backend: RefCell::new(Backend::start()) } }

    pub fn cleanup(&self)
    {
        if let Some(backend) = self.m_backend.borrow_mut().take()
        {
            backend.stop();
        }
    }

    pub fn playing(&self, sourcename: &str) -> bool
    {
        let backend: std::cell::Ref<'_, Option<Backend>> = self.m_backend.borrow();
        let Some(backend) = backend.as_ref()
        else
        {
            return false;
        };

        let voice: Option<Voice> = worker::lock(&backend.m_attached).get(sourcename).copied();
        voice.is_some_and(|voice| backend.m_engine.playing(voice))
    }

    pub fn play(&self, sourcename: &str) { self.send(Command::Play(sourcename.to_owned())); }

    // a stream starts this many seconds in and a static source plays from its start
    pub fn playFrom(&self, sourcename: &str, seconds: f32)
    {
        self.send(Command::PlayFrom(sourcename.to_owned(), seconds));
    }

    pub fn stop(&self, sourcename: &str) { self.send(Command::Stop(sourcename.to_owned())); }

    pub fn setVolume(&self, sourcename: &str, value: f32)
    {
        self.send(Command::SetVolume(sourcename.to_owned(), value));
    }

    pub fn setPitch(&self, sourcename: &str, value: f32) { self.send(Command::SetPitch(sourcename.to_owned(), value)); }

    pub fn backgroundMusic(&self, sourcename: &str, url: &str, identifier: &str, toLoop: bool)
    {
        let play: Play = place(true, toLoop, [0.0, 0.0, 0.0], ATTENUATION_NONE, 0.0);
        self.sendSource(true, sourcename, url, identifier, play);
        self.play(sourcename);
    }

    pub fn newSource(
        &self,
        priority: bool,
        sourcename: &str,
        url: &str,
        identifier: &str,
        toLoop: bool,
        x: f32,
        y: f32,
        z: f32,
        attModel: i32,
        distOrRoll: f32,
    )
    {
        let play: Play = place(priority, toLoop, [x, y, z], attModel, distOrRoll);
        self.sendSource(false, sourcename, url, identifier, play);
    }

    pub fn newStreamingSource(
        &self,
        priority: bool,
        sourcename: &str,
        url: &str,
        identifier: &str,
        toLoop: bool,
        x: f32,
        y: f32,
        z: f32,
        attModel: i32,
        distOrRoll: f32,
    )
    {
        let play: Play = place(priority, toLoop, [x, y, z], attModel, distOrRoll);
        self.sendSource(true, sourcename, url, identifier, play);
    }

    pub fn setListenerPosition(&self, x: f32, y: f32, z: f32) { self.send(Command::ListenerPosition([x, y, z])); }

    pub fn setListenerOrientation(&self, lookX: f32, lookY: f32, lookZ: f32, upX: f32, upY: f32, upZ: f32)
    {
        self.send(Command::ListenerOrientation([lookX, lookY, lookZ], [upX, upY, upZ]));
    }

    fn sendSource(&self, stream: bool, name: &str, url: &str, identifier: &str, play: Play)
    {
        self.send(Command::NewSource(SourceSpec {
            m_name: name.to_owned(),
            m_stream: stream,
            m_url: url.to_owned(),
            m_identifier: identifier.to_owned(),
            m_play: play,
        }));
    }

    fn send(&self, command: Command)
    {
        let backend: std::cell::Ref<'_, Option<Backend>> = self.m_backend.borrow();
        let Some(backend) = backend.as_ref()
        else
        {
            return;
        };

        if let Err(error) = backend.m_commands.send(command)
        {
            log::warn!("Sound command dropped: {error}");
        }
    }
}

impl Default for SoundSystem
{
    fn default() -> Self { Self::new() }
}
