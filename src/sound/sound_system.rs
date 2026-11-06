use super::mixer::{self, ATTENUATION_NONE, Mixer, Params, SharedMixer};
use super::output;
use super::worker::{self, Command, SourceSpec};
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

struct Engine
{
    m_mixer: SharedMixer,
    m_commands: Sender<Command>,
    m_worker: JoinHandle<()>,
    m_output: cpal::Stream,
}

impl Engine
{
    fn start() -> Option<Engine>
    {
        let mixer: SharedMixer = Arc::new(Mutex::new(Mixer::new()));
        let output: cpal::Stream = match output::open(mixer.clone())
        {
            Ok(output) => output,
            Err(error) =>
            {
                eprintln!("Sound output unavailable, sound stays silent: {error}");
                return None;
            }
        };
        let (commands, receiver): (Sender<Command>, Receiver<Command>) = mpsc::channel();
        let workerMixer: SharedMixer = mixer.clone();
        let spawned: std::io::Result<JoinHandle<()>> = std::thread::Builder::new()
            .name("Sound Command Thread".to_owned())
            .spawn(move || worker::run(workerMixer, receiver));
        match spawned
        {
            Ok(worker) => Some(Engine { m_mixer: mixer, m_commands: commands, m_worker: worker, m_output: output }),
            Err(error) =>
            {
                eprintln!("Could not start the sound thread, sound stays silent: {error}");
                None
            }
        }
    }

    fn stop(self)
    {
        let Engine { m_mixer: _, m_commands, m_worker, m_output } = self;
        drop(m_output);
        drop(m_commands);
        if m_worker.join().is_err()
        {
            eprintln!("Sound thread panicked during cleanup");
        }
    }
}

pub struct SoundSystem
{
    m_engine: RefCell<Option<Engine>>,
}

impl SoundSystem
{
    pub fn new() -> SoundSystem { SoundSystem { m_engine: RefCell::new(Engine::start()) } }

    pub fn cleanup(&self)
    {
        if let Some(engine) = self.m_engine.borrow_mut().take()
        {
            engine.stop();
        }
    }

    pub fn playing(&self, sourcename: &str) -> bool
    {
        self.m_engine.borrow().as_ref().is_some_and(|engine| mixer::lock(&engine.m_mixer).isPlaying(sourcename))
    }

    pub fn play(&self, sourcename: &str) { self.send(Command::Play(sourcename.to_owned())); }

    pub fn stop(&self, sourcename: &str) { self.send(Command::Stop(sourcename.to_owned())); }

    pub fn setVolume(&self, sourcename: &str, value: f32)
    {
        self.send(Command::SetVolume(sourcename.to_owned(), value));
    }

    pub fn setPitch(&self, sourcename: &str, value: f32) { self.send(Command::SetPitch(sourcename.to_owned(), value)); }

    pub fn backgroundMusic(&self, sourcename: &str, url: &str, identifier: &str, toLoop: bool)
    {
        let params: Params = Params::new([0.0, 0.0, 0.0], ATTENUATION_NONE, 0.0, toLoop);
        self.sendSource(true, true, sourcename, url, identifier, params);
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
        let params: Params = Params::new([x, y, z], attModel, distOrRoll, toLoop);
        self.sendSource(priority, false, sourcename, url, identifier, params);
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
        let params: Params = Params::new([x, y, z], attModel, distOrRoll, toLoop);
        self.sendSource(priority, true, sourcename, url, identifier, params);
    }

    pub fn setListenerPosition(&self, x: f32, y: f32, z: f32) { self.send(Command::ListenerPosition([x, y, z])); }

    pub fn setListenerOrientation(&self, lookX: f32, lookY: f32, lookZ: f32, upX: f32, upY: f32, upZ: f32)
    {
        self.send(Command::ListenerOrientation([lookX, lookY, lookZ], [upX, upY, upZ]));
    }

    fn sendSource(&self, priority: bool, stream: bool, name: &str, url: &str, identifier: &str, params: Params)
    {
        self.send(Command::NewSource(SourceSpec {
            m_name: name.to_owned(),
            m_priority: priority,
            m_stream: stream,
            m_url: url.to_owned(),
            m_identifier: identifier.to_owned(),
            m_params: params,
        }));
    }

    fn send(&self, command: Command)
    {
        let engine: std::cell::Ref<'_, Option<Engine>> = self.m_engine.borrow();
        let Some(engine) = engine.as_ref()
        else
        {
            return;
        };
        if let Err(error) = engine.m_commands.send(command)
        {
            eprintln!("Sound command dropped: {error}");
        }
    }
}

impl Default for SoundSystem
{
    fn default() -> Self { Self::new() }
}
