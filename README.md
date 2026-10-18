# Hush

A tiny Windows app that cleans up your microphone. It removes background noise, steady hum and echo, and it works in every program automatically: Discord, Zoom, Teams, browsers, games, OBS and anything else.

## Install (about 1 minute)

1. Go to **[Releases](../../releases/latest)** and download **Hush-Setup.exe**.
2. Run it.
   - If Windows says *"Windows protected your PC"*, click **More info → Run anyway**. Hush isn't code-signed yet, which is why Windows warns about it.
   - When Windows asks to install the audio driver, click **Install**.
3. Restart if the installer asks you to.

That's it. Hush starts with Windows and makes itself your microphone, so every app hears the cleaned-up version with no settings to change. Quit Hush and your normal mic comes straight back.

**Quick check:** open any online mic test, or the Windows Sound settings, and talk. Then turn on a fan or type loudly to hear the difference.

## The sliders

The defaults already sound good. If you want to fine-tune:

| Slider | What it does |
| --- | --- |
| Microphone volume | How loud your voice is. |
| Noise removal | Removes sudden sounds: typing, clicks, dogs, traffic. Too high can sound robotic. |
| Background hum removal | Removes steady sounds: fans, air conditioners, electrical buzz. |
| Echo removal | Stops people hearing themselves or your speakers through your mic. You can turn it down if you wear headphones. |
| Silence between words | Mutes the mic while you are not talking. |
| Even out my volume | Keeps your voice at a steady level. |

Tip: turn on **Let me hear myself** with headphones to tune the sliders by ear.

## Uninstall

Use Windows **Settings → Apps → Hush → Uninstall**. Your normal microphone is put back automatically. VB-CABLE stays installed in case other apps use it. You can remove it separately from the same Apps list.

## How it works

Hush cleans your real mic with [sonora](https://crates.io/crates/sonora) (WebRTC echo, noise and volume processing) and [nnnoiseless](https://crates.io/crates/nnnoiseless) (RNNoise AI noise removal). It sends the result into the free [VB-CABLE](https://vb-audio.com/Cable/) virtual microphone, which the installer downloads for you. Then it makes that virtual mic the Windows default.

## Building

```
cargo test --release      # sound-cleaning code, runs on any OS
cargo build --release     # on Windows: target/release/hush.exe
ISCC.exe installer\hush.iss   # Inno Setup: dist/Hush-Setup.exe
```

Pushing a tag like `v0.1.0` makes GitHub Actions publish a release with the installer attached.
