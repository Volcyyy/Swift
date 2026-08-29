
## About

Swift is a rebuild of the Destiny 2 timer/tracker written in Rust and Svelte that displays stats about your current activity and clear history.

Swift v1.4.1

A customizable Destiny 2 activity timer and tracker for Windows.

What's new in v1.4.1:

Fixes only, no new features. Your settings, themes and layout are untouched.

- **The timer now appears within a few seconds of loading in, instead of taking up to two minutes.** It always showed the correct elapsed time once it arrived, but it arrived very late. Measured on a lost sector: 132 seconds before, around 3 seconds after.
- Every request to Bungie was being redirected before it was answered, doubling the round trips.
- Connections are reused instead of being rebuilt for every request, and requests now time out, so a stalled connection can no longer freeze updates.
- Fetching your clear counts no longer holds up the timer.
- A single failed request while starting up no longer leaves the app stuck on an error screen.
- The timer corrects for drift between your PC clock and Bungie's, so a wrong system clock no longer skews it.
- The Spotify companion now shuts down with the app instead of being left running.

<details>
<summary>What was actually causing the delay</summary>

Bungie's API is behind a CDN that was serving current-activity data minted up to three minutes earlier, while reporting it as seconds old. Bungie's servers had the data within about a second the whole time. The current-activity request now varies its cache key so it reaches them directly; everything else still uses the cache, and polling was slowed from once to twice per second, so the app makes fewer requests overall than it did before.

`cargo run --example probe -- mint` reproduces the measurement.

</details>

What's new in v1.4.0:
- Overlay scaling (75%-150%)
- Adjustable overlay opacity
- Top Left, Top Right, Bottom Left and Bottom Right positioning
- Custom X/Y position offsets
- Individual Timer, Daily Clears, Weekly Clears and Spotify toggles
- New preset themes:
  - Swift Default
  - Ice
  - Void
  - Crimson
  - Emerald
  - Monochrome
- Various UI and customization improvements

Spotify integration is optional and requires your own Spotify Developer Client ID.

Developed by Volc <3

**If you want to use the overlay, make sure that you play Destiny 2 in borderless windowed or windowed fullscreen mode.**

## Features
- No sign-in required (your account needs to be public though)
- Non-overlay window for those who play in pure fullscreen mode
- Fast-updating and accurate API timer
- Displays your daily and weekly clear count
- Notifications displaying the results of your last clear
- Custom location and colors
- Easy-to-access configuration and account-switching
- Runs in the background and is manageable via the tray icon
- Spotify integration

## Installation

Lastest release in https://github.com/Volcyyy/Swift/releases

Read the README file it explains everything!

If you have any problems with installing or using swift, contact vvolc on discord

## Building

Releases compile a Bungie API key into the binary, so set it before building:

```powershell
$env:BUNGIE_API_KEY = "<your key>"
npx tauri build
```

A release build refuses to compile without one rather than producing an app that fails for every user at runtime. Debug builds don't need it: `consts::api_key()` reads `BUNGIE_API_KEY` from the environment at runtime too, so `npx tauri dev` works with the key simply set in your shell.

Get a key at https://www.bungie.net/en/Application

## Acknowledgements
Using the source code from Threepole
