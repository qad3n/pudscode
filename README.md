# pudscode

A small general-purpose sound engine for games. It plays decoded sounds and streamed music through [cpal](https://crates.io/crates/cpal), decodes Ogg Vorbis with [lewton](https://crates.io/crates/lewton) and, with the `wav` feature, WAV with [hound](https://crates.io/crates/hound). It needs nothing extra installed on Linux or Windows. Without an output device the engine stays silent and logs one line.

```rust
use pudscode::{Bus, Engine, Play, Settings, Sound, Source};
use std::time::Duration;

let engine: Engine = Engine::new(Settings::new());
let music: Bus = engine.addBus();
let step: Sound = Sound::load(&Source::File("sounds/step.ogg".into()))?;
engine.play(&step, Play { m_volume: 0.8, m_pitch: 1.1, ..Play::new() });
let song = engine.stream(&Source::File("music/day.ogg".into()), Play {
    m_bus: music,
    m_looping: true,
    m_fadeIn: Duration::from_secs(1),
    ..Play::new()
})?;
engine.setBusVolume(music, 0.5, Duration::from_millis(200));
```

- `Sound` is a fully decoded clip, cheap to clone and play many times. `Engine::stream` decodes on a background thread while playing, for music and long ambience.
- `Play` sets the bus, volume, pitch, looping, priority, fade in, start offset and an optional position. Positioned voices pan around the listener (`setListenerPosition`, `setListenerOrientation`) and fade with `Attenuation`. A 2D game can leave the listener at its default and use x and y.
- Every call returns a `Voice` handle for `stop` with a fade, `setVolume`, `setPitch`, `setPosition` and `playing`. A handle whose sound ended or was taken over does nothing.
- Buses group voices under the main bus, each with its own volume and fades.
- `Settings` sets the voice and stream counts, stream buffering and the volume and pitch limits. When every voice is busy, a new sound takes over the next voice without priority.
- `Source::Bytes` plays audio from memory, for archives or files a game transforms first.

## paulscode compatibility

The `paulscode` feature adds `pudscode::sound`, a copy of the parts of paulscode SoundSystem with LWJGL OpenAL that Minecraft Beta 1.7.3 relies on, built on the engine above:

- 28 normal and 4 streaming channels, chosen and stolen by the same rules as `Library.getNextChannel`
- linear distance fade (attenuation model 2), volume clamped to 0..1 and pitch to 0.5..2
- streaming in 3 buffers of 131072 bytes, loaded sounds cached per file
- commands run on a worker thread, so `playing` lags right after `play`, as in paulscode

Codecs are registered with `SoundSystemConfig::setCodec`. `SoundSystemConfig::registerStreamFilter` lets a codec class transform the file bytes before Vorbis decoding, which is how a client adds formats such as `.mus`. WAV is not supported there.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your
option.
