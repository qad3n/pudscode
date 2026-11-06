mod decode;
mod math;
mod mixer;
mod output;
mod sound_system;
mod sound_system_config;
mod worker;

pub mod codecs
{
    pub struct CodecJOrbis;

    impl CodecJOrbis
    {
        pub const CLASS_NAME: &'static str = "paulscode.sound.codecs.CodecJOrbis";
    }

    pub struct CodecWav;

    impl CodecWav
    {
        pub const CLASS_NAME: &'static str = "paulscode.sound.codecs.CodecWav";
    }
}

pub mod libraries
{
    pub struct LibraryLWJGLOpenAL;

    impl LibraryLWJGLOpenAL
    {
        pub const CLASS_NAME: &'static str = "paulscode.sound.libraries.LibraryLWJGLOpenAL";
    }
}

pub use sound_system::SoundSystem;
pub use sound_system_config::{SoundSystemConfig, StreamFilter};
