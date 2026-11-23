use super::codecs::CodecJOrbis;
use super::sound_system_config::{SoundSystemConfig, StreamFilter};
use crate::decode::{DecodeError, Sound, Source};
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum LoadError
{
    #[error("no codec found for file '{0}'")]
    NoCodec(String),
    #[error("codec {0} is not supported")]
    Unsupported(String),
    #[error("'{0}' is not a file url")]
    Url(String),
    #[error("could not read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

pub fn sound(url: &str, identifier: &str) -> Result<Sound, LoadError> { Ok(Sound::load(&source(url, identifier)?)?) }

// reads the whole file like paulscode and runs the codec stream filter over it
pub fn source(url: &str, identifier: &str) -> Result<Source, LoadError>
{
    let codecClass: String =
        SoundSystemConfig::codecClass(identifier).ok_or_else(|| LoadError::NoCodec(identifier.to_owned()))?;
    let filter: Option<StreamFilter> = SoundSystemConfig::streamFilter(&codecClass);
    if filter.is_none() && codecClass != CodecJOrbis::CLASS_NAME
    {
        return Err(LoadError::Unsupported(codecClass));
    }

    let path: PathBuf = urlPath(url)?;
    let bytes: Vec<u8> = std::fs::read(&path).map_err(|error| LoadError::Io(path, error))?;
    let bytes: Vec<u8> = match filter
    {
        Some(filter) => filter(url, bytes),
        None => bytes,
    };
    Ok(Source::Bytes { name: identifier.into(), bytes: bytes.into() })
}

fn urlPath(url: &str) -> Result<PathBuf, LoadError>
{
    let escapeLength: usize = 3;
    let hexRadix: u32 = 16;
    let invalid = || LoadError::Url(url.to_owned());
    let encoded: &[u8] = url.strip_prefix("file:").ok_or_else(invalid)?.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(encoded.len());
    let mut index: usize = 0;
    while index < encoded.len()
    {
        if encoded[index] == b'%'
        {
            let hex: &[u8] = encoded.get(index + 1..index + escapeLength).ok_or_else(invalid)?;
            let hex: &str = std::str::from_utf8(hex).map_err(|_| invalid())?;
            decoded.push(u8::from_str_radix(hex, hexRadix).map_err(|_| invalid())?);
            index += escapeLength;
        }
        else
        {
            decoded.push(encoded[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map(PathBuf::from).map_err(|_| invalid())
}
