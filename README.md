# Black Desert Discord Rich Presence

Discord Rich Presence for Black Desert. It reads only the files the game writes to disk, never its memory.

## Features

- Shows what you are doing: starting up, main menu, server select, character creation, character select, loading or in game.
- Shows your family, character, server and region.
- Shows your character's class, level and portrait from the Adventurer Profile.
- An elapsed timer for the session or the current phase.
- An Adventurer Profile button, plus one custom button.
- Every line of text is a template you can edit per phase in the settings window.
- Interface built on Material Design 3, with Material You dynamic color.

## Build

Windows only. Needs a Rust toolchain and the MSVC build tools.

```bash
cargo build --release
```

## Usage

Run it and it sits in the system tray. It shows your presence while the game is running and clears it when the game closes.

- Left-click the tray icon to open the settings.
- Right-click it for **Run at Startup**, **Reload Config** and **Quit**.
- The icon is gray when the game is not running, amber when Discord is not connected or the config has an error, and green when the presence is live.

## Adventurer Profile

The Adventurer Profile hides its details by default. Your characters' names, classes and portraits are always shown, but their levels, your guild, gear score, energy, contribution points and life skills stay hidden until you make them public. Sign in on your region's website, open your Adventurer Profile, click **Change Privacy Settings** and turn on **Show Additional Info**. When they are hidden, the tray's log says so and links your profile.

## Regions

| Region | Status |
| --- | --- |
| Asia (TH/SEA) | Supported |
| NA/EU/OC, South America, Korea, Japan, Taiwan/Hong Kong/Macau, Russian-speaking, Turkey/MENA | Not yet |
| Console (Xbox/PS) | Not possible, there is no PC client to read |

In a region that is not yet supported, the presence should still work, without that region's server names or anything from the Adventurer Profile. Nothing has been tested there.

## Limits

- The game only reports server and character IDs, so it asks you to name each new server and character, and suggests the known names.
- Start it before entering a character. Otherwise it shows your main character until you switch.
- One account per PC works best. With several, the family name is taken from whichever account played most recently, so set **Family Name** in the settings, and the character ID suggestions list every account's characters.
