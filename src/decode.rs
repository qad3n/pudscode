use lewton::VorbisError;
use lewton::inside_ogg::OggStreamReader;
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;

const MAX_CHANNELS: usize = 2;
const I16_SCALE: f32 = 32768.0;

mod magic
{
    pub const LENGTH: usize = 4;
    pub const OGG: &[u8; LENGTH] = b"OggS";
    #[cfg(feature = "wav")]
    pub const WAV: &[u8; LENGTH] = b"RIFF";
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError
{
    #[error("could not read {0}: {1}")]
    Io(String, std::io::Error),
    #[error("vorbis decode failed: {0}")]
    Vorbis(#[from] VorbisError),
    #[cfg(feature = "wav")]
    #[error("wav decode failed: {0}")]
    Wav(#[from] hound::Error),
    #[cfg(feature = "wav")]
    #[error("seek failed: {0}")]
    Seek(std::io::Error),
    #[error("{0} channels are not supported")]
    Channels(usize),
    #[error("{0} is not a supported audio format")]
    Format(String),
}

trait Media: Read + Seek + Send {}

impl<T: Read + Seek + Send> Media for T {}

type Reader = Box<dyn Media>;

#[derive(Clone, Debug)]
pub enum Source
{
    Bytes
    {
        name: Arc<str>,
        bytes: Arc<[u8]>,
    },
    File(PathBuf),
}

impl Source
{
    fn open(&self) -> Result<Reader, DecodeError>
    {
        match self
        {
            Source::Bytes { bytes, .. } => Ok(Box::new(Cursor::new(bytes.clone()))),
            Source::File(path) =>
            {
                let file: File = File::open(path).map_err(|error| DecodeError::Io(self.to_string(), error))?;
                Ok(Box::new(BufReader::new(file)))
            }
        }
    }
}

impl fmt::Display for Source
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        match self
        {
            Source::Bytes { name, .. } => formatter.write_str(name),
            Source::File(path) => write!(formatter, "{}", path.display()),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Format
{
    pub m_channels: usize,
    pub m_rate: u32,
}

#[derive(Clone)]
pub struct Sound
{
    m_format: Format,
    m_samples: Arc<[f32]>,
}

impl Sound
{
    pub fn load(source: &Source) -> Result<Sound, DecodeError>
    {
        let mut decoder: Decoder = Decoder::open(source)?;
        let mut samples: Vec<f32> = Vec::new();
        while !decoder.readPacket(&mut samples)?
        {}
        Ok(Sound { m_format: decoder.format(), m_samples: samples.into() })
    }

    pub fn duration(&self) -> f32
    {
        let frames: usize = self.m_samples.len() / self.m_format.m_channels;
        frames as f32 / self.m_format.m_rate as f32
    }

    pub(crate) fn format(&self) -> Format { self.m_format }

    pub(crate) fn samples(&self) -> Arc<[f32]> { self.m_samples.clone() }
}

enum Codec
{
    Vorbis(Box<OggStreamReader<Reader>>),
    #[cfg(feature = "wav")]
    Wav(hound::WavReader<Reader>),
}

pub struct Decoder
{
    m_codec: Codec,
    m_format: Format,
}

impl Decoder
{
    pub fn open(source: &Source) -> Result<Decoder, DecodeError>
    {
        let mut reader: Reader = source.open()?;
        let mut header: [u8; magic::LENGTH] = [0; magic::LENGTH];
        let io = |error: std::io::Error| DecodeError::Io(source.to_string(), error);
        reader.read_exact(&mut header).map_err(io)?;
        reader.seek(SeekFrom::Start(0)).map_err(io)?;
        let decoder: Decoder = match &header
        {
            magic::OGG => Self::vorbis(reader)?,
            #[cfg(feature = "wav")]
            magic::WAV => Self::wav(reader)?,
            _ => return Err(DecodeError::Format(source.to_string())),
        };
        let channels: usize = decoder.m_format.m_channels;
        if channels == 0 || channels > MAX_CHANNELS
        {
            return Err(DecodeError::Channels(channels));
        }

        Ok(decoder)
    }

    pub fn format(&self) -> Format { self.m_format }

    // lewton lands on the start of the page holding the sample so every listener seeking the same spot hears the same
    pub fn seek(&mut self, seconds: f32) -> Result<(), DecodeError>
    {
        let frame: u64 = (f64::from(seconds.max(0.0)) * f64::from(self.m_format.m_rate)) as u64;
        match &mut self.m_codec
        {
            Codec::Vorbis(reader) => reader.seek_absgp_pg(frame)?,
            #[cfg(feature = "wav")]
            Codec::Wav(reader) => reader.seek(frame.min(u64::from(u32::MAX)) as u32).map_err(DecodeError::Seek)?,
        }
        Ok(())
    }

    // returns true once the source has no samples left
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

    fn vorbis(reader: Reader) -> Result<Decoder, DecodeError>
    {
        let reader: OggStreamReader<Reader> = OggStreamReader::new(reader)?;
        let format: Format = Format {
            m_channels: usize::from(reader.ident_hdr.audio_channels),
            m_rate: reader.ident_hdr.audio_sample_rate,
        };
        Ok(Decoder { m_codec: Codec::Vorbis(Box::new(reader)), m_format: format })
    }

    #[cfg(feature = "wav")]
    fn wav(reader: Reader) -> Result<Decoder, DecodeError>
    {
        let reader: hound::WavReader<Reader> = hound::WavReader::new(reader)?;
        let spec: hound::WavSpec = reader.spec();
        let format: Format = Format { m_channels: usize::from(spec.channels), m_rate: spec.sample_rate };
        Ok(Decoder { m_codec: Codec::Wav(reader), m_format: format })
    }

    fn readPacket(&mut self, samples: &mut Vec<f32>) -> Result<bool, DecodeError>
    {
        match &mut self.m_codec
        {
            Codec::Vorbis(reader) =>
            {
                let Some(packet) = reader.read_dec_packet_itl()?
                else
                {
                    return Ok(true);
                };
                samples.extend(packet.iter().map(|&sample| sample as f32 / I16_SCALE));
                Ok(false)
            }
            #[cfg(feature = "wav")]
            Codec::Wav(reader) => readWav(reader, samples),
        }
    }
}

#[cfg(feature = "wav")]
fn readWav(reader: &mut hound::WavReader<Reader>, samples: &mut Vec<f32>) -> Result<bool, DecodeError>
{
    let packetSamples: usize = 4096;
    let spec: hound::WavSpec = reader.spec();
    let before: usize = samples.len();
    match spec.sample_format
    {
        hound::SampleFormat::Float =>
        {
            for sample in reader.samples::<f32>().take(packetSamples)
            {
                samples.push(sample?);
            }
        }
        hound::SampleFormat::Int =>
        {
            let scale: f32 = (1u64 << (spec.bits_per_sample - 1)) as f32;
            for sample in reader.samples::<i32>().take(packetSamples)
            {
                samples.push(sample? as f32 / scale);
            }
        }
    }
    Ok(samples.len() - before < packetSamples)
}
