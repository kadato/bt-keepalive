# BT KeepAlive

[![CI](https://github.com/kadato/bt-keepalive/actions/workflows/ci.yml/badge.svg)](https://github.com/kadato/bt-keepalive/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/kadato/bt-keepalive?label=download)](https://github.com/kadato/bt-keepalive/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Keep Bluetooth headphones awake on Windows 10/11**: a small system-tray app that plays quiet audio (or periodic silent pulses) so your headset stays connected and you never miss the first second of a notification, video, or call.

---

## Download

No Python or install wizard required: grab the portable `.exe` and run it.

| Download | Link |
|----------|------|
| **BTKeepAlive.exe** (recommended) | [**Download latest release**](https://github.com/kadato/bt-keepalive/releases/latest/download/BTKeepAlive.exe) |
| Checksum file | [SHA256SUMS.txt](https://github.com/kadato/bt-keepalive/releases/latest/download/SHA256SUMS.txt) |
| All versions | [GitHub Releases](https://github.com/kadato/bt-keepalive/releases) |

### Quick install

1. Download **BTKeepAlive.exe** using the link above.
2. Move it to a permanent folder, for example `%LOCALAPPDATA%\Programs\BTKeepAlive\`.
3. Double-click to run. A tray icon appears: **blue** when playing, **gray** when paused.
4. Right-click the tray icon → **Launch at startup** if you want it every boot.

> **SmartScreen / antivirus:** Unsigned builds may show a Windows SmartScreen prompt. Choose **More info** → **Run anyway**, or build locally (see [Build from source](#build-from-source)).

### Verify download (optional)

```powershell
Get-FileHash "$env:USERPROFILE\Downloads\BTKeepAlive.exe" -Algorithm SHA256
```

Compare the hash with `SHA256SUMS.txt` on the [release page](https://github.com/kadato/bt-keepalive/releases/latest).

---

## Why use this?

Many Bluetooth headphones power down their radio after a short period of silence. When something new plays, the first moment can be clipped while the link reconnects. BT KeepAlive sends a continuous near-inaudible signal, or a periodic quiet pulse, so Windows keeps the audio path active.

Inspired by tools like [SoundKeeper](https://github.com/amd/SoundKeeper), but focused on a simple tray experience with multiple noise presets and a mostly-silent pulse mode.

---

## Features

- **Noise presets**: white, pink, brown, blue, violet
- **40 Hz binaural beats** with adjustable carrier (100–300 Hz)
- **Volume control**: slider dialog plus tray presets
- **Pulse keepalive**: short quiet pulse on an interval (mostly silent, like SoundKeeper)
- **Launch at startup**: registry entry when running the built `.exe`
- **System tray**: play/pause, pulse mode, sound, volume, binaural carrier, startup toggle, quit

---

## Using the app

Right-click the tray icon to change sound, volume, and mode. Default is **brown noise** at **2%** volume, usually inaudible at normal listening distance.

| Tray action | What it does |
|-------------|--------------|
| **Play / Pause** | Start or stop the keepalive stream (label shows **Pause** while playing) |
| **Pulse keepalive** | Toggle pulse mode on/off (checked when pulse mode is active) |
| **Sound** | Pick a noise preset or **40 Hz binaural** (selecting a preset switches back to continuous mode) |
| **Volume** | **Adjust volume…** opens a slider dialog; below that, radio presets (0.01%–20%) |
| **Binaural carrier** | Carrier frequency for **40 Hz binaural**: 100, 150, 200, 250, or 300 Hz |
| **Launch at startup** | Add/remove from Windows startup; enabled only when running the `.exe` |
| **Quit** | Exit the app |

### Continuous vs pulse mode

- **Continuous (default)**: plays your chosen preset at low volume. Best when a tiny background hum is acceptable.
- **Pulse**: sends a very short, very quiet pulse every ~55 seconds. Best when you want the tray icon active but almost no audible output.

If Bluetooth still drops in pulse mode, lower `pulse_interval_sec` in `config.json`.

---

## Config and logs

Settings live in `%APPDATA%\BTKeepAlive\`:

| File | Purpose |
|------|---------|
| `config.json` | Preset, volume, pulse timing, autoplay, etc. |
| `app.log` | General application log (rotating) |
| `audio-errors.log` | Audio/stream errors (rotating) |

Set `BTKEEPALIVE_LOG_LEVEL=DEBUG` for verbose logging.

<details>
<summary><strong>Config reference</strong></summary>

| Key | Default | Tray |
|-----|---------|------|
| `preset` | `brown` | Sound submenu |
| `volume` | `0.02` | Volume / Adjust volume… |
| `carrier_hz` | `200` | Binaural carrier |
| `keepalive_mode` | `continuous` | Pulse keepalive toggle |
| `pulse_interval_sec` | `55` | JSON only (pulse mode timing) |
| `pulse_duration_sec` | `1` | JSON only |
| `pulse_amplitude` | `0.0001` | JSON only |
| `sample_rate` | `44100` | JSON only |
| `buffer_seconds` | `0.012` | JSON only |
| `autoplay` | `true` | JSON / `--no-autoplay` |
| `launch_at_startup` | `false` | Launch at startup (.exe only) |
| `playing` | `true` | Play / Pause |

</details>

---

## Troubleshooting

| Problem | What to try |
|---------|-------------|
| No sound / app exits on start | Check Windows default playback device; see `%APPDATA%\BTKeepAlive\audio-errors.log` |
| “Already running” | Another instance is in the tray; use **Quit** or Task Manager |
| Startup toggle fails | Run as your normal user; check `app.log` |
| Antivirus blocks the `.exe` | One-file PyInstaller builds are sometimes flagged; allowlist the folder or [build locally](#build-from-source) |
| SmartScreen warning | **More info** → **Run anyway** for unsigned builds |

---

## For developers

**Requirements:** Windows 10/11, Python 3.11+

```powershell
git clone https://github.com/kadato/bt-keepalive.git
cd bt-keepalive
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -e ".[dev]"
python -m btkeepalive
```

> **Note:** “Launch at startup” only works with the built `.exe`, not `python -m btkeepalive`.

### CLI

```powershell
python -m btkeepalive --version
python -m btkeepalive --config-path
python -m btkeepalive --no-autoplay
```

### Build from source

```powershell
.\.venv\Scripts\Activate.ps1
pip install -e ".[dev]"
.\build.ps1
# Output: dist\BTKeepAlive.exe
```

Optional installer (requires [Inno Setup 6](https://jrsoftware.org/isinfo.php)):

```powershell
iscc installer\BTKeepAlive.iss
# Output: dist\BTKeepAlive-setup.exe
```

### Tests and lint

```powershell
pip install -e ".[dev]"
ruff check .
ruff format --check .
pytest
```

### Publishing a release

Push a version tag to trigger the GitHub Actions release workflow:

```powershell
git tag v1.1.0
git push origin v1.1.0
```

The workflow builds `BTKeepAlive.exe`, writes `SHA256SUMS.txt`, and attaches both to a GitHub Release.

---

## License

MIT. See [LICENSE](LICENSE).
