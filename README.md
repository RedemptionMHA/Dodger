# Dodger

A fast-paced 2D space shooter written in Rust with [macroquad](https://macroquad.rs). Pilot your ship, dodge falling enemies, and blast them out of the sky for points, all in front of a scrolling shader-powered starfield.

## Features

- **Dodge-and-shoot gameplay**: enemies fall from the top of the screen, and you can either avoid them or shoot them down.
- **Rising difficulty**: spawn rate and enemy speed both scale with your score.
- **Three enemy sizes**: small, medium and big enemies, each with its own animated sprite and explosion effect.
- **Animated sprites**: ship banking animations when you steer left or right, flapping enemies, and a pulsing laser bolt.
- **Particle explosions**: every kill spawns a spritesheet-based particle burst, bigger enemies make bigger explosions.
- **GLSL starfield background**: a custom shader rendered to a small render target and scaled up for a retro look. The stars drift as you steer.
- **Sound and music**: looping 8-bit soundtrack, laser and explosion sound effects.
- **Persistent high score**: saved to disk between sessions.

## Controls

| Action | Keys |
| --- | --- |
| Move | `W` `A` `S` `D` or arrow keys |
| Shoot | `Space` (hold to fire, 0.3 s cooldown) |
| Start game / continue | `Space` |
| Toggle fullscreen (menu only) | `F` |

## Scoring

- You earn points continuously for every second you stay alive.
- Destroying an enemy awards bonus points equal to its size, so bigger enemies are worth more.
- Colliding with any enemy ends the run.
- Your best score is saved automatically.

## Getting started

### Requirements

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain)
- Platform dependencies for macroquad. On Linux you may need ALSA and X11/OpenGL development packages, see the [macroquad docs](https://github.com/not-fl3/macroquad#linux).

### Run

```bash
git clone <your-repo-url>
cd dodger
cargo run --release
```

### Test

```bash
cargo test
```

The tests cover high-score path construction and saving/loading the high score.

## Project layout

```
dodger/
├── Cargo.toml
├── assets/
│   ├── ship.png
│   ├── enemy-small.png
│   ├── enemy-medium.png
│   ├── enemy-big.png
│   ├── Bolt.png
│   ├── 01.png, 02.png, 03.png      # explosion spritesheets (6 frames each)
│   ├── 8bit-spaceshooter.ogg
│   ├── explosion.wav
│   └── laser.wav
└── src/
    ├── main.rs
    └── starfield-shader.glsl
```

The `assets` folder is looked up next to the executable first, then in the current working directory, then in the project directory, so a release build can be launched from anywhere as long as `assets` ships alongside it.

## Where your data is saved

The high score is stored in `Save/highscore.dat`, in the same directory as the executable. Delete that file to reset your high score.

## How it works

- **State machine**: the game moves between `Menu`, `Tutorial` (first launch only), `Playing` and `GameOver`.
- **Sprites**: a small `AnimatedSprite` helper steps through spritesheet rows and frames at a set FPS.
- **Collisions**: simple axis-aligned rectangle overlap tests for player vs. enemies and bullets vs. enemies.
- **Resources**: all textures and sounds load in a coroutine behind a "Loading resources..." screen and are kept in macroquad's global storage.
- **Effects**: explosions use `macroquad_particles`, and the background uses a custom vertex and fragment shader.

## Acknowledgments

Built while following the macroquad tutorial series at [mq.agical.se](https://mq.agical.se).
