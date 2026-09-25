# CHANGELOG

<!-- version list -->

## v2.1.0 (2026-09-25)

### Features

- Install updates from the Settings window. The update banner has an
  Install update button instead of a second check button.
- Show release notes under What's new in the update banner.
- Keep a failed update attempt visible in the banner until the next
  check or install.

## v2.0.2 (2026-09-25)

### Bug Fixes

- Publish `BTKeepAlive.exe` and `WebView2Loader.dll` to the GitHub
  Release. The in-app updater downloads the bare exe plus
  `SHA256SUMS.txt` and hot swaps it, so without these assets every
  update check failed with `release is missing exe or checksum assets`.

## v2.0.1 (2026-09-25)

### Bug Fixes

- Run the release build as a GUI app so Windows does not open a
  terminal window. Debug builds keep the console for logs.

## v2.0.0 (2026-09-25)

### Features

- Rewrite the app in Rust for Windows 10 and 11
- Add a Settings window with status, sound, volume, quiet mode, and system sections
- Flatten the tray menu to Play, Settings, Check for updates, Open logs, and Quit
- Close the audio stream between pulse bursts to cut wakeups
- Report device switches through system events instead of polling
- Render the tray icon in code with no image dependency at runtime
- Export 5 seconds of audio with `--render-wav` for headless checks

### Breaking Changes

- Drop `sample_rate` and `buffer_seconds` from `config.json`. The engine
  renders at the device mix rate with system-managed buffering. Old files
  load fine; both keys disappear on the next save.
- Replace `audio-errors.log` with entries in `app.log`.

## v1.4.4 (2026-06-19)

### Bug Fixes

- **updater**: Reset environment variables before self-restart
  ([`952d7d1`](https://github.com/kadato/bt-keepalive/commit/952d7d19012f4334ec941f803bd0638a39848f5c))

### Chores

- **ci**: Add pre-commit config and compile step to CI
  ([`96c3402`](https://github.com/kadato/bt-keepalive/commit/96c3402678228fc795405aa09d97f79a6c00d1f3))

### Code Style

- **updater**: Ruff formatting and import sorting in test_updater
  ([`f21af8b`](https://github.com/kadato/bt-keepalive/commit/f21af8b07b9c0f1d3219626c64b8af0cc7990a04))


## v1.4.3 (2026-06-19)

### Bug Fixes

- **stream**: Recover audio stream on unexpected failure or device change
  ([`e18e161`](https://github.com/kadato/bt-keepalive/commit/e18e1615d4f9b088765cce53189371799264d5c5))

### Build System

- **deps**: Bump actions/checkout from 4 to 7
  ([`0384d73`](https://github.com/kadato/bt-keepalive/commit/0384d73a58588526c9a898e34b8b23323eccbdf2))

### Code Style

- **stream**: Fix ruff lint and formatting issues
  ([`ac89f02`](https://github.com/kadato/bt-keepalive/commit/ac89f02a538f71dc85c65d8641281fdc1dbb9c5f))

### Testing

- **stream**: Add tests for monitor loop recovery and stream recreation
  ([`f07641e`](https://github.com/kadato/bt-keepalive/commit/f07641edd99de5cdc87d2475f283a864a2200fb3))


## v1.4.2 (2026-06-09)

### Bug Fixes

- **updater**: Check if active executable exists before update and hot-swap
  ([`5da1bed`](https://github.com/kadato/bt-keepalive/commit/5da1bede95aceda21e6159c7c320c078c2f802d3))

### Testing

- **updater**: Add unit tests for missing/moved executable scenarios
  ([`8b090fc`](https://github.com/kadato/bt-keepalive/commit/8b090fcf75953a3d88c16eb1e6378d899d6cc135))


## v1.4.1 (2026-06-07)

### Bug Fixes

- **build**: Add PIL._imaging to hiddenimports in spec file
  ([`1796552`](https://github.com/kadato/bt-keepalive/commit/1796552584e12b467bb02e082969812c4520f9b9))


## v1.4.0 (2026-06-07)

### Bug Fixes

- **ci**: Resolve ruff lint and format issues
  ([`c3ee936`](https://github.com/kadato/bt-keepalive/commit/c3ee936ca18fc8dfa696ff744953b5e9b8b53471))

### Features

- **app**: Check for updates in background loop at startup and periodically
  ([`f6a22c1`](https://github.com/kadato/bt-keepalive/commit/f6a22c12e453bb2d5e1cd642082813c4e92d3db5))

- **ui**: Conditionally show update menu option in tray UI
  ([`d2464df`](https://github.com/kadato/bt-keepalive/commit/d2464df5f3359bd4f098c0744c0c719f39141f0b))

- **updater**: Refactor updater logic and add check_for_update_available
  ([`fcb704b`](https://github.com/kadato/bt-keepalive/commit/fcb704bf31dd15c4cc90fefcb6d0e79a56a6a642))


## v1.3.0 (2026-06-06)

### Bug Fixes

- **stream**: Generate silent sine wave for pulse keepalive instead of noise
  ([`59c4cca`](https://github.com/kadato/bt-keepalive/commit/59c4cca8964d976b736c66f05beb77ca47165ad5))

- **stream**: Stop and close audio stream outside lock to prevent deadlock on exit
  ([`88afe75`](https://github.com/kadato/bt-keepalive/commit/88afe759e28795f6b4382d07188331280f9d5441))

### Code Style

- **tests**: Remove trailing whitespace in test_stream_blocksize.py
  ([`c08c158`](https://github.com/kadato/bt-keepalive/commit/c08c1586ffde902ea6b26be7a1499cebd28683e8))

### Features

- **device-monitor**: Implement default Windows audio endpoint ID detection
  ([`8ad6384`](https://github.com/kadato/bt-keepalive/commit/8ad63844e81a30e79fb663d92a79c31df146fcb9))

- **stream**: Restart audio stream when default audio device changes
  ([`04a038d`](https://github.com/kadato/bt-keepalive/commit/04a038d5c1671f872de63f0956ee4fec069daee9))

- **updater**: Implement github update checking, verification, and hot-swap logic
  ([`de14f2b`](https://github.com/kadato/bt-keepalive/commit/de14f2bab945f2159321ceea026b636d810cccdc))

- **updater**: Integrate update checking with application startup and tray menu
  ([`9db16d4`](https://github.com/kadato/bt-keepalive/commit/9db16d4b857b831a51fa43c7a3af60194891f475))


## v1.2.1 (2026-06-05)

### Bug Fixes

- **stream**: Check active status and recreate inactive sounddevice streams
  ([`4f0aa91`](https://github.com/kadato/bt-keepalive/commit/4f0aa913edaaa2795a900884dbb80cc9af0ca493))

### Continuous Integration

- Configure python-semantic-release and add to dev dependencies
  ([`5d6eea6`](https://github.com/kadato/bt-keepalive/commit/5d6eea658ae5a4194e125d91fd9bcb5b8b93cff2))

- Update GitHub Actions dependency versions
  ([`96c6df1`](https://github.com/kadato/bt-keepalive/commit/96c6df1857206e7ad10ad23308ee1d2314d31f4f))

- Update release workflow to automate release process on push to main
  ([`21072ef`](https://github.com/kadato/bt-keepalive/commit/21072ef82cf72b71d70b8d96b76a9b6157ca868e))

### Testing

- **stream**: Add test for stream active state and auto-recreation
  ([`514babe`](https://github.com/kadato/bt-keepalive/commit/514babe55505d1a3d53584b9d047e9f6230040f8))

- **stream**: Fix lint errors in test_stream_state.py
  ([`66e8417`](https://github.com/kadato/bt-keepalive/commit/66e8417905498f18127a5f31b215ddbcd70b5e7a))


## v1.2.0 (2026-06-04)

### Build System

- Disable UPX compression in PyInstaller spec
  ([`bc5ae9e`](https://github.com/kadato/bt-keepalive/commit/bc5ae9e2c9e5db4444843a6c198b55a1417e73cd))

### Chores

- Consolidate dev install and expand gitignore
  ([`394c4f6`](https://github.com/kadato/bt-keepalive/commit/394c4f6ebcf9b44ea54568a129cc351ebaaa3222))

- **release**: Bump version to 1.2.0
  ([`2f5f979`](https://github.com/kadato/bt-keepalive/commit/2f5f979066183daef0e3e8fc2af2c7ce1b96d969))

### Code Style

- Normalize punctuation in UI strings and comments
  ([`d6cb30c`](https://github.com/kadato/bt-keepalive/commit/d6cb30cc7a693e7e02a65d6414c005bf69b39319))

### Continuous Integration

- Add quality gates to release workflow and dependabot
  ([`4ab2f98`](https://github.com/kadato/bt-keepalive/commit/4ab2f98d087accaad2803e26fe9dc6781c0c1161))

- Install editable package in workflow
  ([`eded7af`](https://github.com/kadato/bt-keepalive/commit/eded7af1bcf540460f07e3fcb45bd16715a0c969))

### Documentation

- Normalize punctuation in README and release notes
  ([`1bced23`](https://github.com/kadato/bt-keepalive/commit/1bced233b20f110ef3da10b38b7c75861ee760d3))

- Simplify tray menu and config documentation
  ([`45a2ffd`](https://github.com/kadato/bt-keepalive/commit/45a2ffd03b3fc986b977c8775234fcd4385e859c))

### Features

- **version**: Read package version from pyproject.toml
  ([`6d0988a`](https://github.com/kadato/bt-keepalive/commit/6d0988a155b55b966f06a9c23d13fd5a708f2d08))

### Refactoring

- Remove silent preset and pulse interval dialog
  ([`3eae01c`](https://github.com/kadato/bt-keepalive/commit/3eae01cc8cd6ac14428d4b09bc8e14fcf6bdb2f0))

- **stream**: Extract blocksize helper and expand pulse tests
  ([`3683074`](https://github.com/kadato/bt-keepalive/commit/3683074ea3ad5debc3be59ccfd5d952ce886e833))

### Testing

- Add app, binaural, startup, and single-instance coverage
  ([`2478a5c`](https://github.com/kadato/bt-keepalive/commit/2478a5c5f9e5ecb17c1ffd04bfe6c88e0ab0918f))


## v1.1.0 (2026-06-04)

- Initial Release
