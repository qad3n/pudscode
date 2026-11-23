use crate::decode::{DecodeError, Decoder, Sound, Source};
use crate::feeder::{self, Feed};
use crate::mixer::{self, Mixer, Pool, Samples, SharedMixer};
use crate::output;
use crate::play::{Bus, Play, Settings, Voice};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

struct Live
{
    m_output: cpal::Stream,
    m_feeds: Sender<Feed>,
    m_feeder: JoinHandle<()>,
}

impl Live
{
    fn start(mixer: &SharedMixer) -> Option<Live>
    {
        let output: cpal::Stream = match output::open(mixer.clone())
        {
            Ok(output) => output,
            Err(error) =>
            {
                log::error!("Sound output unavailable, sound stays silent: {error}");
                return None;
            }
        };
        let (feeds, receiver): (Sender<Feed>, Receiver<Feed>) = mpsc::channel();
        let feederMixer: SharedMixer = mixer.clone();
        let spawned: std::io::Result<JoinHandle<()>> = std::thread::Builder::new()
            .name("Sound Stream Thread".to_owned())
            .spawn(move || feeder::run(feederMixer, receiver));
        match spawned
        {
            Ok(feeder) => Some(Live { m_output: output, m_feeds: feeds, m_feeder: feeder }),
            Err(error) =>
            {
                log::error!("Could not start the sound thread, sound stays silent: {error}");
                None
            }
        }
    }

    fn stop(self)
    {
        let Live { m_output, m_feeds, m_feeder } = self;
        // the device stops pulling before the feeder goes away
        drop(m_output);
        drop(m_feeds);
        match m_feeder.join()
        {
            Ok(()) => log::debug!("Sound engine stopped"),
            Err(_) => log::error!("Sound thread panicked during cleanup"),
        }
    }
}

pub struct Engine
{
    m_mixer: SharedMixer,
    m_live: Option<Live>,
}

impl Engine
{
    // falls back to silent when no output device opens
    pub fn new(settings: Settings) -> Engine
    {
        let mixer: SharedMixer = Arc::new(Mutex::new(Mixer::new(settings)));
        let live: Option<Live> = Live::start(&mixer);
        Engine { m_mixer: mixer, m_live: live }
    }

    pub fn silent(settings: Settings) -> Engine
    {
        Engine { m_mixer: Arc::new(Mutex::new(Mixer::new(settings))), m_live: None }
    }

    pub fn audible(&self) -> bool { self.m_live.is_some() }

    pub fn play(&self, sound: &Sound, play: Play) -> Option<Voice>
    {
        self.m_live.as_ref()?;
        self.mixer().start(&play, sound.format(), Samples::Static(sound.samples()), Pool::Normal)
    }

    // decodes on the stream thread while playing so long music never sits in memory decoded
    pub fn stream(&self, source: &Source, play: Play) -> Result<Option<Voice>, DecodeError>
    {
        let Some(live) = self.m_live.as_ref()
        else
        {
            return Ok(None);
        };

        let mut decoder: Decoder = Decoder::open(source)?;
        if play.m_startAt > 0.0
        {
            decoder.seek(play.m_startAt)?;
        }

        let started: Option<Voice> = self.mixer().start(&play, decoder.format(), Samples::stream(), Pool::Stream);
        let Some(voice) = started
        else
        {
            return Ok(None);
        };

        if let Err(error) = live.m_feeds.send(Feed::new(voice, source.clone(), play.m_looping, decoder))
        {
            log::warn!("Sound stream dropped: {error}");
        }
        Ok(Some(voice))
    }

    pub fn playing(&self, voice: Voice) -> bool { self.mixer().playing(voice) }

    pub fn stop(&self, voice: Voice, fade: Duration) { self.mixer().stop(voice, fade); }

    pub fn setVolume(&self, voice: Voice, volume: f32) { self.mixer().setVolume(voice, volume); }

    pub fn setPitch(&self, voice: Voice, pitch: f32) { self.mixer().setPitch(voice, pitch); }

    pub fn setPosition(&self, voice: Voice, position: [f32; 3]) { self.mixer().setPosition(voice, position); }

    pub fn addBus(&self) -> Bus { self.mixer().addBus() }

    pub fn setBusVolume(&self, bus: Bus, volume: f32, fade: Duration) { self.mixer().setBusVolume(bus, volume, fade); }

    pub fn setListenerPosition(&self, position: [f32; 3]) { self.mixer().setListenerPosition(position); }

    pub fn setListenerOrientation(&self, at: [f32; 3], up: [f32; 3]) { self.mixer().setListenerOrientation(at, up); }

    fn mixer(&self) -> MutexGuard<'_, Mixer> { mixer::lock(&self.m_mixer) }
}

impl Drop for Engine
{
    fn drop(&mut self)
    {
        if let Some(live) = self.m_live.take()
        {
            live.stop();
        }
    }
}
