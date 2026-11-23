use crate::decode::{Decoder, Source};
use crate::mixer::{self, Mixer, SharedMixer};
use crate::play::{Settings, Voice};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub struct Feed
{
    m_voice: Voice,
    m_source: Source,
    m_looping: bool,
    m_decoder: Decoder,
}

impl Feed
{
    pub fn new(voice: Voice, source: Source, looping: bool, decoder: Decoder) -> Feed
    {
        Feed { m_voice: voice, m_source: source, m_looping: looping, m_decoder: decoder }
    }

    // keeps chunks queued ahead of the mixer and returns false once the voice is gone or the source ran out
    fn feed(&mut self, mixer: &Mutex<Mixer>, chunk: usize, chunks: usize) -> bool
    {
        let Some(queued) = mixer::lock(mixer).streamQueued(self.m_voice)
        else
        {
            return false;
        };
        if queued >= chunk * chunks
        {
            return true;
        }

        let mut samples: Vec<f32> = Vec::with_capacity(chunk);
        let ended: bool = match self.m_decoder.read(&mut samples, chunk)
        {
            Ok(ended) => ended && !self.restart(),
            Err(error) =>
            {
                log::error!("Error streaming {}: {error}", self.m_source);
                true
            }
        };
        mixer::lock(mixer).appendStream(self.m_voice, &samples, ended);
        !ended
    }

    fn restart(&mut self) -> bool
    {
        if !self.m_looping
        {
            return false;
        }

        match Decoder::open(&self.m_source)
        {
            Ok(decoder) =>
            {
                self.m_decoder = decoder;
                true
            }
            Err(error) =>
            {
                log::error!("Error restarting {}: {error}", self.m_source);
                false
            }
        }
    }
}

pub fn run(mixer: SharedMixer, feeds: Receiver<Feed>)
{
    let settings: Settings = mixer::lock(&mixer).settings();
    let mut active: Vec<Feed> = Vec::new();
    loop
    {
        match feeds.recv_timeout(POLL_INTERVAL)
        {
            Ok(feed) => active.push(feed),
            Err(RecvTimeoutError::Timeout) =>
            {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        active.retain_mut(|feed| feed.feed(&mixer, settings.m_streamChunk, settings.m_streamChunks));
    }
}
