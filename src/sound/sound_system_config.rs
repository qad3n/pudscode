use std::sync::{Mutex, MutexGuard, PoisonError};

pub type StreamFilter = fn(&str, Vec<u8>) -> Vec<u8>;

struct Codec
{
    m_extension: String,
    m_className: String,
}

struct Filter
{
    m_className: &'static str,
    m_filter: StreamFilter,
}

struct Config
{
    m_codecs: Vec<Codec>,
    m_filters: Vec<Filter>,
}

static CONFIG: Mutex<Config> = Mutex::new(Config { m_codecs: Vec::new(), m_filters: Vec::new() });

fn config() -> MutexGuard<'static, Config> { CONFIG.lock().unwrap_or_else(PoisonError::into_inner) }

pub struct SoundSystemConfig;

impl SoundSystemConfig
{
    pub fn addLibrary(_libraryClass: &str) {}

    pub fn setCodec(extension: &str, codecClass: &str)
    {
        let codec: Codec = Codec { m_extension: extension.to_ascii_lowercase(), m_className: codecClass.to_owned() };
        config().m_codecs.push(codec);
    }

    pub fn registerStreamFilter(codecClass: &'static str, filter: StreamFilter)
    {
        config().m_filters.push(Filter { m_className: codecClass, m_filter: filter });
    }

    pub(super) fn codecClass(filename: &str) -> Option<String>
    {
        let filename: String = filename.to_ascii_lowercase();
        let config: MutexGuard<'static, Config> = config();
        let codec: Option<&Codec> = config.m_codecs.iter().find(|codec| filename.ends_with(&codec.m_extension));
        codec.map(|codec| codec.m_className.clone())
    }

    pub(super) fn streamFilter(codecClass: &str) -> Option<StreamFilter>
    {
        config().m_filters.iter().find(|filter| filter.m_className == codecClass).map(|filter| filter.m_filter)
    }
}
