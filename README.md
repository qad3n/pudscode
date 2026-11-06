# pudscode

A small sound engine with the API of paulscode SoundSystem, the library Minecraft Beta 1.7.3 plays its sounds through. It is built on [cpal](https://crates.io/crates/cpal) for device output and [lewton](https://crates.io/crates/lewton) for Vorbis decoding, so it needs nothing extra installed on Linux or Windows.

It copies the parts of paulscode with LWJGL OpenAL that the game relies on:

- 28 normal and 4 streaming channels, chosen and stolen by the same rules as `Library.getNextChannel`
- linear distance fade (attenuation model 2), volume clamped to 0..1 and pitch to 0.5..2
- streaming in 3 buffers of 131072 bytes, loaded sounds cached per file
- commands run on a worker thread, so `playing` lags right after `play`, as in paulscode

Mono sounds are panned with equal-power stereo panning, which approximates OpenAL. Stereo sounds play unpanned. Without an output device the engine stays silent and logs one line.

Codecs are registered with `SoundSystemConfig::setCodec`. Ogg Vorbis is decoded directly. `SoundSystemConfig::registerStreamFilter` lets a codec class transform the file bytes before Vorbis decoding, which is how a client adds formats such as `.mus`. WAV is not supported.
