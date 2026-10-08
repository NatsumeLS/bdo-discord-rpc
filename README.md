# Black Desert Discord Rich Presence

Discord Rich Presence for Black Desert. It reads the files the game writes to disk. With Npcap installed, it also watches the game's network traffic. It never reads the game's memory and never sends anything to the game.

## Features

- Shows what you are doing: starting up, main menu, server select, character creation, character select, loading or in game.
- Shows your family, character, server and region.
- Shows where you are, node and territory, with Npcap installed.
- Shows your character's class, level and portrait from the Adventurer Profile.
- An elapsed timer for the session or the current phase.
- Two buttons, the first linking your Adventurer Profile by default.
- Every line of text and both buttons are templates you can edit per phase.
- Material Design 3 interface, light or dark, colored from an accent color you pick.

## Installation

Windows only.

1. Download the latest zip from [Releases](https://github.com/NatsumeLS/bdo-discord-rpc/releases/latest).
2. Unzip it into a folder you can write to, since the config and log are saved next to the exe.
3. Run `bdo-discord-rpc.exe`.
4. Optional: install [Npcap](https://npcap.com/#download) to also show your location.

## Usage

It sits in the system tray. It shows your presence while the game runs, and clears it when the game closes.

- Left-click the tray icon to open or close the settings.
- Right-click it for **Run at Startup**, **Reload Config** and **Quit**.
- The icon color shows the state:
  - Gray: the game is not running, or the app is switched off in the settings.
  - Amber: Discord is not connected, or the config has an error.
  - Green: the presence is live.

## Packet Capture

[Npcap](https://npcap.com/#download) is optional. With it, the app also listens to the game's own connection, which gives it:

- Your location, as `{territory}` and `{node}` in the templates.
- The server, character and family as soon as you enter the world.
- Your character's in-game name, so you are not asked to name it.
- Readings that keep working after the game's log file hits its size limit.

It only listens, and never sends anything. Without Npcap, everything else still works from the game's files.

Game patches can move what the capture reads. The app keeps its data in a `data` folder next to the exe, and when the game updates to a version the app does not know yet, it downloads the newest data from this repository into that folder, so it keeps working without an app update. Deleting the folder resets it to the data the app shipped with.

## Adventurer Profile

Your characters' names, classes and portraits are always shown. Levels, guild, gear score, energy, contribution points and life skills stay hidden until you make them public:

1. Sign in on your region's website.
2. Open your Adventurer Profile.
3. Click **Change Privacy Settings**.
4. Turn on **Show Additional Info**.

## Connections

The app only connects to:

- Discord on your own PC, to set the presence.
- Your region's Adventurer Profile website, to find and read your profile, at most once an hour.
- This repository on GitHub, only after a game patch the app does not know yet, to download whichever of its data files are newer.
- The game's own server names, looked up by DNS with Npcap installed, to name the server you are on.

It sends nothing else anywhere.

## Regions

| Region | Status |
| --- | --- |
| Asia (TH/SEA) | Supported |
| NA/EU/OC, South America, Korea, Japan, Taiwan/Hong Kong/Macau, Russian-speaking, Turkey/MENA | Not yet |
| Console (Xbox/PS) | Not possible |

In a region that is not supported yet, the presence should still work, without server names or the Adventurer Profile.

## Limits

- New servers have to be named once, and the app suggests the known names. Without Npcap, new characters have to be named too.
- Start it before entering a character. Otherwise it shows your main character until you switch character or, with Npcap, change channel.
- With several accounts on one PC, set **Family Name** in the settings.

## Uninstalling

1. Right-click the tray icon and untick **Run at Startup**, so no startup entry is left behind.
2. Quit it from the same menu.
3. Delete its folder.

## Building

Run the setup script once. It installs Rust, the MSVC build tools and the [Npcap SDK](https://npcap.com/#download), and is safe to run again.

```powershell
./scripts/setup.ps1
cargo build --release
```

### Location Data

The node, region and territory data in `assets/client/` comes from the game's own data files. It is refreshed by the maintainer after patches that add areas, and reaches the app the same way.
