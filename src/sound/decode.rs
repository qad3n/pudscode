use super::codecs::CodecJOrbis;
use super::sound_system_config::{SoundSystemConfig, StreamFilter};
use lewton::VorbisError;
use lewton::inside_ogg::OggStreamReader;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

const MAX_CHANNELS: u8 = 2;
const I16_SCALE: f32 = 32768.0;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError
{
    #[error("no codec found for file '{0}'")]
    NoCodec(String),
    #[error("codec {0} is not supported")]
    Unsupported(String),
    #[error("'{0}' is not a file url")]
    Url(String),
    #[error("could not read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("vorbis decode failed: {0}")]
    Vorbis(#[from] VorbisError),
    #[error("{0} channels are not supported")]
    Channels(u8),
}

#[derive(Clone, Copy)]
pub struct Format
{
    pub m_channels: usize,
    pub m_rate: u32,
}

pub struct Pcm
{
    pub m_format: Format,
    pub m_samples: Arc<[f32]>,
}

pub struct Decoder
{
    m_reader: OggStreamReader<Cursor<Vec<u8>>>,
    m_format: Format,
}

impl Decoder
{
    pub fn open(url: &str, identifier: &str) -> Result<Decoder, DecodeError>
    {
        let reader: OggStreamReader<Cursor<Vec<u8>>> = OggStreamReader::new(Cursor::new(readBytes(url, identifier)?))?;
        let channels: u8 = reader.ident_hdr.audio_channels;
        if channels == 0 || channels > MAX_CHANNELS
        {
            return Err(DecodeError::Channels(channels));
        }
        let format: Format = Format { m_channels: channels as usize, m_rate: reader.ident_hdr.audio_sample_rate };
        Ok(Decoder { m_reader: reader, m_format: format })
    }

    pub fn format(&self) -> Format { self.m_format }

    pub fn read(&mut self, samples: &mut Vec<f32>, minSamples: usize) -> Result<bool, DecodeError>
    {
        let target: usize = samples.len() + minSamples;
        while samples.len() < target
        {
            if self.readPacket(samples)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn readPacket(&mut self, samples: &mut Vec<f32>) -> Result<bool, DecodeError>
    {
        let Some(packet) = self.m_reader.read_dec_packet_itl()?
        else
        {
            return Ok(true);
        };
        samples.extend(packet.iter().map(|&sample| sample as f32 / I16_SCALE));
        Ok(false)
    }
}

pub fn decodeAll(url: &str, identifier: &str) -> Result<Pcm, DecodeError>
{
    let mut decoder: Decoder = Decoder::open(url, identifier)?;
    let mut samples: Vec<f32> = Vec::new();
    while !decoder.readPacket(&mut samples)?
    {}
    Ok(Pcm { m_format: decoder.format(), m_samples: samples.into() })
}

fn readBytes(url: &str, identifier: &str) -> Result<Vec<u8>, DecodeError>
{
    let codecClass: String =
        SoundSystemConfig::codecClass(identifier).ok_or_else(|| DecodeError::NoCodec(identifier.to_owned()))?;
    let filter: Option<StreamFilter> = SoundSystemConfig::streamFilter(&codecClass);
    if filter.is_none() && codecClass != CodecJOrbis::CLASS_NAME
    {
        return Err(DecodeError::Unsupported(codecClass));
    }
    let path: PathBuf = urlPath(url)?;
    let bytes: Vec<u8> = std::fs::read(&path).map_err(|error| DecodeError::Io(path, error))?;
    Ok(match filter
    {
        Some(filter) => filter(url, bytes),
        None => bytes,
    })
}

fn urlPath(url: &str) -> Result<PathBuf, DecodeError>
{
    let escapeLength: usize = 3;
    let hexRadix: u32 = 16;
    let invalid = || DecodeError::Url(url.to_owned());
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
