# Hush

A tiny Windows app that cleans up your microphone: it removes background noise, steady hum and echo, and it works in every program (Discord, Zoom, Teams, browsers, games, OBS...).

## How to use it

1. **Install VB-CABLE once** (free): <https://vb-audio.com/Cable/>. It adds a virtual microphone called *CABLE Output*. Restart your PC afterwards.
2. **Run `hush.exe`.** The defaults already sound good. Leave it running.
3. In the program you talk with, choose **CABLE Output** as the microphone.
   Or click **Make "CABLE Output" my default mic** in Hush so most programs use it automatically.

Closing the window keeps Hush running in the taskbar. Use **Quit Hush** to stop it.

## The sliders

| Slider | What it does |
| --- | --- |
| Microphone volume | How loud your voice is. |
| Noise removal | Removes sudden sounds: typing, clicks, dogs, traffic. Too high can sound robotic. |
| Background hum removal | Removes steady sounds: fans, air conditioners, electrical buzz. |
| Echo removal | Stops people hearing themselves or your speakers through your mic. Turn down with headphones. |
| Silence between words | Mutes the mic while you are not talking. |
| Even out my volume | Keeps your voice at a steady level. |

Tip: turn on **Let me hear myself** with headphones to tune the sliders by ear.

## Building

```
cargo build --release     # on Windows; produces target/release/hush.exe
cargo test --release      # the sound-cleaning code is tested on any OS
```

Under the hood: [sonora](https://crates.io/crates/sonora) (WebRTC echo/noise/gain) and [nnnoiseless](https://crates.io/crates/nnnoiseless) (RNNoise), both BSD-3.
