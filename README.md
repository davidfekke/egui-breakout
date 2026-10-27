# Brick / Break

A playable Breakout game written entirely in Rust using egui + eframe 0.36.2.
The game field, bricks, paddle, ball, trails and particles all use egui's Painter.
There is no JavaScript game or separate game engine underneath.

## Run from source

Install a current stable Rust toolchain (https://rustup.rs), then in this folder:

```sh
cargo run --release
```

The source targets macOS, Windows and Linux through eframe. On this machine the
macOS (x86_64 + arm64) and Linux x86_64 builds below were compiled and the unit
tests run; the Windows binaries are produced by the GitHub Actions workflow or
by the `cargo-xwin` cross build. This project does not include web deployment.

## Release builds

The game uses native windowing, audio and GPU APIs, so each platform needs its
own toolchain. Supported targets:

| Platform            | Rust target                 | Typical build machine |
| ------------------- | --------------------------- | --------------------- |
| Windows x64         | `x86_64-pc-windows-msvc`    | Windows x64           |
| Windows ARM64       | `aarch64-pc-windows-msvc`   | Windows ARM64         |
| Linux x64           | `x86_64-unknown-linux-gnu`  | Ubuntu 24.04          |
| Linux ARM64         | `aarch64-unknown-linux-gnu` | Ubuntu 24.04 ARM64    |
| macOS x64 (Intel)   | `x86_64-apple-darwin`       | macOS Intel           |
| macOS ARM64 (extra) | `aarch64-apple-darwin`      | macOS Apple Silicon   |

### GitHub Actions (recommended)

`.github/workflows/build.yml` builds every target above on GitHub runners, then
packs `brick-break-windows-x64.zip`, `brick-break-windows-arm64.zip`,
`brick-break-linux-x64.tar.gz`, `brick-break-linux-arm64.tar.gz` and
`brick-break-macos-x64.zip` (plus an Apple Silicon build, which most modern Macs
need). Pushing a `v*` tag attaches the archives to a GitHub release; otherwise
run it by hand from the Actions tab.

```sh
git init && git add . && git commit -m "Brick / Break"
gh repo create egui-breakout --private --source . --push
git tag v0.1.0 && git push --tags
```

The macOS jobs go one step further and produce a real `Brick Break.app` bundle
through `scripts/make-macos-app.sh`.

### Building locally from this Mac

macOS x64 (verified on this machine):

```sh
rustup target add x86_64-apple-darwin
cargo build --release --target x86_64-apple-darwin
./scripts/make-macos-app.sh x86_64-apple-darwin   # -> dist/Brick Break.app
```

A universal binary is also easy, since Apple's SDK ships both architectures:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
./scripts/make-macos-app.sh universal             # lipo + bundle
```

Windows x64 and ARM64, cross-compiled with the MSVC ABI ([cargo-xwin](https://github.com/rust-cross/cargo-xwin)):

```sh
brew install llvm
cargo install --locked cargo-xwin
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc
cargo xwin build --release --target aarch64-pc-windows-msvc
```

`cargo-xwin` downloads the Microsoft CRT and Windows SDK into `~/.xwin-cache`
on first use, so Visual Studio is not required. The executables land in
`target/<triple>/release/egui-breakout.exe`. Building this on Windows (or in CI)
is less fiddly, but the cross build produces the same MSVC binaries.

Linux x64 and ARM64 through Docker, which provides the ALSA headers that
`rodio -> cpal` needs to link:

```sh
./scripts/build-linux-docker.sh amd64   # x86_64-unknown-linux-gnu
./scripts/build-linux-docker.sh arm64   # aarch64-unknown-linux-gnu
```

The script builds inside `rust:1.99-bookworm`; the resulting binary needs glibc
2.35 at most (verified with `objdump -T`), so it runs on Debian 12+ and Ubuntu
22.04+. On an Apple Silicon Mac the arm64 build runs natively and the amd64
build uses Docker's emulation; on Linux hosts, `chown` the `target/` directory
afterwards if Docker ran as root. To support an older distro, build in
`rust:1.99-bullseye` (glibc 2.31) instead.

`cargo-zigbuild` and `cross` are alternatives, but both need the same ALSA
development package in the target sysroot, which is why the container route is
simplest.

### Runtime requirements and notes

- Linux needs `libasound2` (audio) and a Vulkan driver such as `libvulkan1` /
  Mesa, plus `libxkbcommon0` with either `libx11-6`+`libxi6` or
  `libwayland-client0`. eframe here is configured with only the `wgpu`
  renderer; add the `glow` feature if you want an OpenGL fallback for very old
  GPUs.
- Windows builds are plain `.exe` files; the release profile sets
  `windows_subsystem = "windows"` so no console window opens next to the game.
- The macOS `.app` is ad-hoc signed but **not notarized**. On another Mac,
  Gatekeeper will require right-click → Open the first time, or
  `xattr -dr com.apple.quarantine "Brick Break.app"`. To ship it properly,
  sign with a Developer ID and run `notarytool`.

## Controls

- Mouse, left/right arrows, or A/D: move the paddle.
- Space or click the field: launch; pause/resume during play.
- P or Escape: pause/resume.
- R or Restart: reset the game.
- M or the Sound button: mute/unmute all audio.
- After clearing a wall: Space/click advances to a faster level.
- Losing window focus pauses a running game.

You start with three lives. Higher rows earn more points. Hit the ball near the
paddle's edges to change its direction. The ball speeds up slightly on paddle
hits, up to a cap. Each level starts faster.

## 8-bit audio

`src/audio.rs` synthesizes all sound at startup with classic console voices:
pulse waves (12.5/25/50% duty), a stepped 4-bit triangle, and LFSR noise. Nothing
is loaded from disk. Effects are quantized to 5-bit amplitude for a crunchy
1980s arcade character:

- a rising pulse sweep when the ball launches
- a punchy blip and triangle thump on paddle hits
- a bright pentatonic two-note "ding" for each brick row (with alternating pitch)
- a soft click off the walls
- a descending four-note sting when a life is lost
- a fanfare on level clear and a slow descending game-over jingle
- rising/falling menu blips for pause and resume

A looping Am–F–C–G chiptune (arpeggio lead, triangle bass, noise hats and snare)
plays quietly while a round is underway and pauses with the game. Audio uses
[rodio](https://github.com/RustAudio/rodio) with only its playback feature, so no
decoder stack is pulled in. If no output device is available the game logs a
warning and runs silently.

## Implementation

`src/main.rs` contains the simulation and native UI. Physics runs in fixed
1/240-second steps, independently of rendering, with a capped frame delta to
avoid huge jumps after stalls. The field maintains a fixed 900×600 logical
coordinate system and scales with the window. Nine unit tests cover walls,
paddle aiming, brick scoring, life loss, level completion, pause, and the audio
synth (levels, bounds, sweeps and loop length).

```sh
cargo test
```

## Choosing a Rust framework

- egui/eframe: well suited to tools, editors and interfaces; its painter is
  sufficient for this small arcade game. Physics and game behavior are custom.
  https://github.com/emilk/egui
- Macroquad: my recommendation for a new small 2D game; a simple game-focused API.
  https://macroquad.rs/
- Bevy: consider for larger 2D/3D projects needing ECS, scenes, assets, animation
  and audio. More architecture to learn than this game requires.
  https://bevy.org/
