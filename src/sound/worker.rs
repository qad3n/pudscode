use super::decode::{self, Decoder, Pcm};
use super::math::Vector;
use super::mixer::{self, Mixer, Params, SharedMixer, SourceData, StreamFile};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};

mod streaming
{
    use std::time::Duration;

    pub const BUFFER_BYTES: usize = 131072;
    pub const BYTES_PER_SAMPLE: usize = 2;
    pub const BUFFERS: usize = 3;
    pub const POLL_INTERVAL: Duration = Duration::from_millis(20);
}

pub struct SourceSpec
{
    pub m_name: String,
    pub m_priority: bool,
    pub m_stream: bool,
    pub m_url: String,
    pub m_identifier: String,
    pub m_params: Params,
}

pub enum Command
{
    NewSource(SourceSpec),
    Play(String),
    PlayFrom(String, f32),
    Stop(String),
    SetVolume(String, f32),
    SetPitch(String, f32),
    ListenerPosition(Vector),
    ListenerOrientation(Vector, Vector),
}

struct Stream
{
    m_voice: usize,
    m_sourceId: u64,
    m_file: StreamFile,
    m_loop: bool,
    m_decoder: Decoder,
}

impl Stream
{
    fn feed(&mut self, mixer: &Mutex<Mixer>) -> bool
    {
        let chunkSamples: usize = streaming::BUFFER_BYTES / streaming::BYTES_PER_SAMPLE;
        let Some(queued) = mixer::lock(mixer).streamQueued(self.m_voice, self.m_sourceId)
        else
        {
            return false;
        };
        if queued >= chunkSamples * streaming::BUFFERS
        {
            return true;
        }
        let mut samples: Vec<f32> = Vec::with_capacity(chunkSamples);
        let ended: bool = match self.m_decoder.read(&mut samples, chunkSamples)
        {
            Ok(ended) => ended && !self.restart(),
            Err(error) =>
            {
                log::error!("Error streaming {}: {error}", self.m_file.m_identifier);
                true
            }
        };
        mixer::lock(mixer).appendStream(self.m_voice, self.m_sourceId, &samples, ended);
        !ended
    }

    fn restart(&mut self) -> bool
    {
        if !self.m_loop
        {
            return false;
        }
        match Decoder::open(&self.m_file.m_url, &self.m_file.m_identifier)
        {
            Ok(decoder) =>
            {
                self.m_decoder = decoder;
                true
            }
            Err(error) =>
            {
                log::error!("Error restarting {}: {error}", self.m_file.m_identifier);
                false
            }
        }
    }
}

struct Worker
{
    m_mixer: SharedMixer,
    m_buffers: HashMap<String, Option<Arc<Pcm>>>,
    m_streams: Vec<Stream>,
}

pub fn run(mixer: SharedMixer, commands: Receiver<Command>)
{
    let mut worker: Worker = Worker { m_mixer: mixer, m_buffers: HashMap::new(), m_streams: Vec::new() };
    loop
    {
        match commands.recv_timeout(streaming::POLL_INTERVAL)
        {
            Ok(command) => worker.execute(command),
            Err(RecvTimeoutError::Timeout) =>
            {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        worker.feedStreams();
    }
}

impl Worker
{
    fn mixer(&self) -> MutexGuard<'_, Mixer> { mixer::lock(&self.m_mixer) }

    fn execute(&mut self, command: Command)
    {
        match command
        {
            Command::NewSource(spec) => self.newSource(spec),
            Command::Play(name) => self.play(&name, 0.0),
            Command::PlayFrom(name, seconds) => self.play(&name, seconds),
            Command::Stop(name) => self.mixer().stop(&name),
            Command::SetVolume(name, volume) => self.mixer().setVolume(&name, volume),
            Command::SetPitch(name, pitch) => self.mixer().setPitch(&name, pitch),
            Command::ListenerPosition(position) => self.mixer().setListenerPosition(position),
            Command::ListenerOrientation(at, up) => self.mixer().setListenerOrientation(at, up),
        }
    }

    fn newSource(&mut self, spec: SourceSpec)
    {
        let data: SourceData = if spec.m_stream
        {
            SourceData::Stream(StreamFile { m_url: spec.m_url, m_identifier: spec.m_identifier })
        }
        else
        {
            let Some(pcm) = self.buffer(&spec.m_url, &spec.m_identifier)
            else
            {
                return;
            };
            SourceData::Static(pcm)
        };
        self.mixer().newSource(spec.m_name, spec.m_priority, spec.m_params, data);
    }

    fn buffer(&mut self, url: &str, identifier: &str) -> Option<Arc<Pcm>>
    {
        let decode = || match decode::decodeAll(url, identifier)
        {
            Ok(pcm) => Some(Arc::new(pcm)),
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
        let opened: Option<(StreamFile, bool)> = self.mixer().streamToOpen(name);
        let Some((file, toLoop)) = opened
        else
        {
            self.mixer().play(name, None);
            return;
        };
        let decoded: Result<Decoder, decode::DecodeError> =
            Decoder::open(&file.m_url, &file.m_identifier).and_then(|mut decoder|
            {
                if start > 0.0
                {
                    decoder.seek(start)?;
                }
                Ok(decoder)
            });
        let decoder: Decoder = match decoded
        {
            Ok(decoder) => decoder,
            Err(error) =>
            {
                log::error!("Error opening stream {}: {error}", file.m_identifier);
                return;
            }
        };
        log::debug!("Streaming {} on {name}", file.m_identifier);
        let started: Option<(usize, u64)> = self.mixer().play(name, Some(decoder.format()));
        if let Some((voice, sourceId)) = started
        {
            self.m_streams.retain(|stream| stream.m_voice != voice);
            let stream: Stream =
                Stream { m_voice: voice, m_sourceId: sourceId, m_file: file, m_loop: toLoop, m_decoder: decoder };
            self.m_streams.push(stream);
        }
    }

    fn feedStreams(&mut self)
    {
        let mixer: &Mutex<Mixer> = &self.m_mixer;
        self.m_streams.retain_mut(|stream| stream.feed(mixer));
    }
}
