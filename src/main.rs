// Main entry point for the Dodger game — a dodge-and-shoot arcade game built with macroquad.

// --- Shader constants ---

// Fragment shader sourced from an external GLSL file for the starfield background effect.
const FRAGMENT_SHADER: &str = include_str!("starfield-shader.glsl");

// Vertex shader for the starfield rendering pass. It transforms vertex positions
// using Model and Projection matrices and passes the time uniform to the fragment shader.
const STARFIELD_VERTEX_SHADER: &str = "#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying float iTime;

uniform mat4 Model;
uniform mat4 Projection;
uniform vec4 _Time;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    iTime = _Time.x;
}
";

// --- Imports ---

use macroquad::prelude::*;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use macroquad_particles::{self as particles, AtlasConfig, Emitter, EmitterConfig};
use macroquad::audio::{load_sound, play_sound, play_sound_once, PlaySoundParams, Sound};
use macroquad::experimental::collections::storage;
use macroquad::experimental::coroutines::start_coroutine;

#[derive(Clone)]
struct Animation {
    row: usize,
    frames: usize,
    fps: u32,
}

struct SpriteFrame {
    dest_size: Vec2,
    source_rect: Rect,
}

struct AnimatedSprite {
    frame_width: f32,
    frame_height: f32,
    animations: Vec<Animation>,
    current_animation: usize,
    current_frame: usize,
    frame_timer: f32,
    looped: bool,
}

impl AnimatedSprite {
    fn new(frame_width: u32, frame_height: u32, animations: &[Animation], looped: bool) -> Self {
        Self {
            frame_width: frame_width as f32,
            frame_height: frame_height as f32,
            animations: animations.to_vec(),
            current_animation: 0,
            current_frame: 0,
            frame_timer: 0.0,
            looped,
        }
    }

    fn set_animation(&mut self, index: usize) {
        if index < self.animations.len() && self.current_animation != index {
            self.current_animation = index;
            self.current_frame = 0;
            self.frame_timer = 0.0;
        }
    }

    fn update(&mut self, delta_time: f32) {
        if self.animations.is_empty() {
            return;
        }

        let animation = &self.animations[self.current_animation];
        self.frame_timer += delta_time;
        let frame_duration = 1.0 / animation.fps as f32;

        while self.frame_timer >= frame_duration {
            self.frame_timer -= frame_duration;
            if self.current_frame + 1 < animation.frames {
                self.current_frame += 1;
            } else if self.looped {
                self.current_frame = 0;
            } else {
                self.current_frame = animation.frames.saturating_sub(1);
            }
        }
    }

    fn frame(&self) -> SpriteFrame {
        let animation = &self.animations[self.current_animation];
        let x = self.current_frame as f32 * self.frame_width;
        let y = animation.row as f32 * self.frame_height;

        SpriteFrame {
            dest_size: vec2(self.frame_width, self.frame_height),
            source_rect: Rect::new(x, y, self.frame_width, self.frame_height),
        }
    }
}

// --- Constants ---

// Filename used for persisting the high score to disk.
const HIGH_SCORE_FILE: &str = "highscore.dat";

// --- Enums ---

/// Represents the top-level game state, controlling which screen/logic is active.
#[derive(PartialEq)]
enum GameState {
    Menu,     // Main menu with options and score history
    Playing,  // Active gameplay
    GameOver, // Game-over overlay
    Tutorial, // First-time tutorial screen
}


// --- Structs ---

/// A rectangular obstacle (cube) in the game world. Used for both falling cubes
/// the player must dodge and bullets the player shoots.
struct Obstacle {
    size: f32,     // Half-extent of the square obstacle
    speed: f32,    // Movement speed in pixels per second
    x: f32,        // Center X position
    y: f32,        // Center Y position
    collided: bool, // Marks this obstacle as already hit (prevents double-processing)
}

/// Returns a particle emitter configuration for explosion effects when a cube is destroyed.
/// `texture` should be one of the 6-frame (32x64) explosion spritesheets.
fn particle_explosion(texture: Texture2D) -> particles::EmitterConfig {
    particles::EmitterConfig {
        local_coords: false,
        one_shot: true,
        emitting: true,
        lifetime: 0.6,
        lifetime_randomness: 0.3,
        explosiveness: 0.65,
        initial_direction_spread: 2.0 * std::f32::consts::PI,
        initial_velocity: 300.0,
        initial_velocity_randomness: 0.8,
        size: 16.0,
        size_randomness: 0.3,
        texture: Some(texture),
        atlas: Some(AtlasConfig::new(6, 1, 0..6)),
        ..Default::default()
    }
}

impl Obstacle {
    /// Computes the axis-aligned bounding rectangle for collision detection.
    fn rect(&self) -> Rect {
        Rect {
            x: self.x - self.size / 2.0,
            y: self.y - self.size / 2.0,
            w: self.size,
            h: self.size,
        }
    }

    /// Checks whether this obstacle's bounding rect overlaps with another's.
    fn collides_with(&self, other: &Obstacle) -> bool {
        self.rect().overlaps(&other.rect())
    }
}

/// The player character — a circle that moves within the screen bounds.
struct Player {
    size: f32,  // Diameter of the player circle
    x: f32,     // Center X position
    y: f32,     // Center Y position
    speed: f32, // Movement speed in pixels per second
}

impl Player {
    /// Computes the axis-aligned bounding rectangle for collision detection.
    fn rect(&self) -> Rect {
        Rect {
            x: self.x - self.size / 2.0,
            y: self.y - self.size / 2.0,
            w: self.size,
            h: self.size,
        }
    }

}

// --- Helper functions ---

/// Constructs the path to the high-score file inside a given base directory.
fn high_score_path_for(base_dir: &Path) -> PathBuf {
    base_dir.join("Save").join(HIGH_SCORE_FILE)
}

/// Resolves the high-score file path by checking the executable directory,
/// then the working directory, and finally falling back to the cargo manifest dir.
fn high_score_path() -> PathBuf {
    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            return high_score_path_for(exe_dir);
        }
    }

    if let Ok(cwd) = env::current_dir() {
        return high_score_path_for(&cwd);
    }

    high_score_path_for(Path::new(env!("CARGO_MANIFEST_DIR")))
}

/// Resolves the assets folder by checking the executable directory first,
/// then the working directory, and finally the cargo manifest dir.
/// This mirrors `high_score_path()` so the release binary can find its
/// assets regardless of the directory it's launched from.
fn assets_dir() -> PathBuf {
    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join("assets");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    if let Ok(cwd) = env::current_dir() {
        let candidate = cwd.join("assets");
        if candidate.exists() {
            return candidate;
        }
    }

    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

/// Loads the high score from disk, defaulting to 0.0 if the file is missing or corrupt.
fn load_high_score() -> f32 {
    let path = high_score_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|content| content.trim().parse::<f32>().ok())
        .unwrap_or(0.0)
}

/// Saves the high score to disk, creating the Save directory if needed.
fn save_high_score(high_score: f32) {
    let path = high_score_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Err(err) = fs::write(&path, high_score.to_string()) {
        eprintln!("Failed to save high score: {err}");
    }
}

// --- Drawing functions ---

/// Renders the main menu screen: title, high score, score history, and settings.
fn draw_menu(score_history: &[f32], high_score: &f32, fullscreen: bool) {
    let title = "DODGER";
    let title_dim = measure_text(title, None, 64, 1.0);

    draw_text(
        title,
        screen_width() / 2.0 - title_dim.width / 2.0,
        screen_height() / 2.0 - 120.0,
        64.0,
        YELLOW,
    );

    let start_text = "Press SPACE to Start";
    let start_dim = measure_text(start_text, None, 32, 1.0);
    draw_text(
        start_text,
        screen_width() / 2.0 - start_dim.width / 2.0,
        screen_height() / 2.0 + 20.0,
        32.0,
        WHITE,
    );

    let settings_title = "Settings";
    draw_text(settings_title, 20.0, 150.0, 24.0, LIGHTGRAY);

    let fullscreen_text = if fullscreen { "Fullscreen: ON" } else { "Fullscreen: OFF" };
    draw_text(fullscreen_text, 20.0, 185.0, 24.0, WHITE);

    draw_text("Press F to toggle fullscreen", 20.0, 210.0, 20.0, LIGHTGRAY);

    let high_text = format!("High Score: {:.1}", high_score);
    draw_text(&high_text, 20.0, 30.0, 28.0, WHITE);

    let history_title = "Score History:";
    draw_text(history_title, 20.0, 70.0, 20.0, LIGHTGRAY);

    let max_history = 5.min(score_history.len());
    for (i, score) in score_history[score_history.len().saturating_sub(max_history)..].iter().enumerate() {
        let history_text = format!("  {}. {:.1}", i + 1, score);
        draw_text(&history_text, 20.0, 100.0 + (i as f32) * 25.0, 18.0, LIGHTGRAY);
    }
}

/// Renders the tutorial screen shown on the player's first launch.
fn draw_tutorial() {
    let title = "HOW TO PLAY";
    let title_dim = measure_text(title, None, 48, 1.0);
    draw_text(
        title,
        screen_width() / 2.0 - title_dim.width / 2.0,
        screen_height() / 2.0 - 120.0,
        48.0,
        YELLOW,
    );

    let instructions = vec![
        "Move: WASD or Arrow Keys",
        "Shoot: SPACE (with cooldown)",
        "Avoid falling cubes!",
        "Destroy cubes for points!",
    ];

    for (i, instruction) in instructions.iter().enumerate() {
        let y = screen_height() / 2.0 - 40.0 + (i as f32) * 40.0;
        draw_text(instruction, screen_width() / 2.0 - 150.0, y, 28.0, WHITE);
    }

    let continue_text = "Press SPACE to Start";
    let continue_dim = measure_text(continue_text, None, 24, 1.0);
    draw_text(
        continue_text,
        screen_width() / 2.0 - continue_dim.width / 2.0,
        screen_height() / 2.0 + 160.0,
        24.0,
        LIGHTGRAY,
    );
}

/// Renders the game-over overlay with score, high score, and restart prompt.
fn draw_gameover_screen(score: &f32, high_score: &f32) {
    let title = "GAME OVER";
    let title_dim = measure_text(title, None, 64, 1.0);
    let box_height = title_dim.height + 150.0;
    let box_y = screen_height() / 2.0 - box_height / 2.0;

    // Semi-transparent backdrop behind the game-over text.
    draw_rectangle(
        0.0,
        box_y,
        screen_width(),
        box_height,
        Color::new(0.0, 0.0, 0.0, 0.6),
    );

    draw_text(
        title,
        screen_width() / 2.0 - title_dim.width / 2.0,
        screen_height() / 2.0 - 40.0,
        64.0,
        RED,
    );

    let score_text = format!("Score: {:.1}", score);
    let score_dim = measure_text(&score_text, None, 28, 1.0);
    draw_text(
        &score_text,
        screen_width() / 2.0 - score_dim.width / 2.0,
        screen_height() / 2.0 + 30.0,
        28.0,
        WHITE,
    );

    let high_text = format!("High Score: {:.1}", high_score);
    let high_dim = measure_text(&high_text, None, 28, 1.0);
    draw_text(
        &high_text,
        screen_width() / 2.0 - high_dim.width / 2.0,
        screen_height() / 2.0 + 70.0,
        28.0,
        LIGHTGRAY,
    );

    let restart_text = "Press SPACE to Return to Menu";
    let restart_dim = measure_text(restart_text, None, 24, 1.0);
    draw_text(
        restart_text,
        screen_width() / 2.0 - restart_dim.width / 2.0,
        screen_height() / 2.0 + 130.0,
        24.0,
        WHITE,
    );
}

// --- Resources ---

/// Holds every asset loaded from the file system: textures, sounds, and
/// the sprite sheets used to animate them.
struct Resources {
    ship_texture: Texture2D,
    enemy_small_texture: Texture2D,
    enemy_medium_texture: Texture2D,
    enemy_big_texture: Texture2D,
    bolt_texture: Texture2D,
    explosion_small_texture: Texture2D,
    explosion_medium_texture: Texture2D,
    explosion_big_texture: Texture2D,
    theme_music: Sound,
    sound_explosion: Sound,
    sound_laser: Sound,
}

impl Resources {
    async fn new() -> Result<Resources, macroquad::Error> {
        let ship_texture: Texture2D = load_texture("ship.png").await?;
        ship_texture.set_filter(FilterMode::Nearest);

        let enemy_small_texture: Texture2D = load_texture("enemy-small.png").await?;
        enemy_small_texture.set_filter(FilterMode::Nearest);

        let enemy_medium_texture: Texture2D = load_texture("enemy-medium.png").await?;
        enemy_medium_texture.set_filter(FilterMode::Nearest);

        let enemy_big_texture: Texture2D = load_texture("enemy-big.png").await?;
        enemy_big_texture.set_filter(FilterMode::Nearest);

        let bolt_texture: Texture2D = load_texture("Bolt.png").await?;
        bolt_texture.set_filter(FilterMode::Nearest);

        let explosion_small_texture: Texture2D = load_texture("01.png").await?;
        explosion_small_texture.set_filter(FilterMode::Nearest);

        let explosion_medium_texture: Texture2D = load_texture("02.png").await?;
        explosion_medium_texture.set_filter(FilterMode::Nearest);

        let explosion_big_texture: Texture2D = load_texture("03.png").await?;
        explosion_big_texture.set_filter(FilterMode::Nearest);

        build_textures_atlas();

        let theme_music = load_sound("8bit-spaceshooter.ogg").await?;
        let sound_explosion = load_sound("explosion.wav").await?;
        let sound_laser = load_sound("laser.wav").await?;

        Ok(Resources {
            ship_texture,
            enemy_small_texture,
            enemy_medium_texture,
            enemy_big_texture,
            bolt_texture,
            explosion_small_texture,
            explosion_medium_texture,
            explosion_big_texture,
            theme_music,
            sound_explosion,
            sound_laser,
        })
    }

    /// Loads all resources in a coroutine while showing a "Loading resources..."
    /// message on screen, then stores the result in Macroquad's global storage
    /// so it can be retrieved from anywhere with `storage::get::<Resources>()`.
    pub async fn load() -> Result<(), macroquad::Error> {
        let resources_loading = start_coroutine(async move {
            let resources = Resources::new().await.unwrap();
            storage::store(resources);
        });
        while !resources_loading.is_done() {
            clear_background(BLACK);
            let text = format!(
                "Loading resources {}",
                ".".repeat(((get_time() * 2.) as usize) % 4)
            );
            draw_text(
                &text,
                screen_width() / 2. - 160.,
                screen_height() / 2.,
                40.,
                WHITE,
            );
            next_frame().await;
        }
        Ok(())
    }
}

// --- Main entry point ---

#[macroquad::main("Dodger")]
async fn main() -> Result<(), macroquad::Error> {
    // Point macroquad at the assets folder using a path resolved from the
    // executable's own location, not the process's current working directory.
    // This must happen before any load_texture/load_sound calls, otherwise
    // it has no effect on them.
    set_pc_assets_folder(assets_dir().to_str().expect("assets path is not valid UTF-8"));
    Resources::load().await?;
    let resources = storage::get::<Resources>();

    // Base movement speed applied to the player and used as a multiplier for bullet speed.
    const BASE_SPEED: f32 = 220.0;
    // Seed the RNG with the current time so obstacle patterns differ each run.
    rand::srand(miniquad::date::now() as u64);

    // --- Initialize game objects ---

    let mut player = Player {
        size: 34.0,
        x: screen_width() / 2.0,
        y: screen_height() * 0.8,
        speed: BASE_SPEED,
    };
    let mut obstacles: Vec<Obstacle> = Vec::new();
    let mut bullets: Vec<Obstacle> = vec![];
    let mut state = GameState::Menu;
    let mut score: f32 = 0.0;
    let mut high_score: f32 = load_high_score();
    let mut score_history: Vec<f32> = Vec::new();
    let mut shot_cooldown: f32 = 0.0;
    let shot_cooldown_duration: f32 = 0.3; // cooldown between shots in seconds
    let mut cubes_destroyed: i32 = 0;
    let mut first_time: bool = true;
    let mut explosions: Vec<(Emitter, Vec2)> = vec![];
    let mut fullscreen = false;
    let mut enemy_small_sprite = AnimatedSprite::new(
        17,
        16,
        &[Animation {
            row: 0,
            frames: 2,
            fps: 12,
        }],
        true,
    );
    let mut enemy_medium_sprite = AnimatedSprite::new(
        32,
        16,
        &[Animation {
            row: 0,
            frames: 2,
            fps: 12,
        }],
        true,
    );
    let mut enemy_big_sprite = AnimatedSprite::new(
        32,
        32,
        &[Animation {
            row: 0,
            frames: 2,
            fps: 12,
        }],
        true,
    );
    let mut ship_sprite = AnimatedSprite::new(
        16,
        24,
        &[
            Animation {
                row: 0,
                frames: 2,
                fps: 12,
            },
            Animation {
                row: 2,
                frames: 2,
                fps: 12,
            },
            Animation {
                row: 4,
                frames: 2,
                fps: 12,
            },
        ],
        true,
    );

    let mut bolt_sprite = AnimatedSprite::new(
        9,
        9,
        &[Animation {
            row: 0,
            frames: 4,
            fps: 12,
        }],
        true,
    );

    // --- Starfield shader setup ---

    // A small render target that the starfield shader draws to, then is scaled
    // up to the full screen each frame for a cheap low-resolution starfield effect.
    let mut direction_modifier: f32 = 0.0;
    let render_target = render_target(320, 150);
    render_target.texture.set_filter(FilterMode::Nearest);
    let material = load_material(
        ShaderSource::Glsl {
            vertex: STARFIELD_VERTEX_SHADER,
            fragment: FRAGMENT_SHADER,
        },
        MaterialParams {
            uniforms: vec![
                UniformDesc::new("iResolution", UniformType::Float2),
                UniformDesc::new("direction_modifier", UniformType::Float1),
            ],
            ..Default::default()
        },
    )?;

    // --- Main game loop ---

    play_sound(
        &resources.theme_music,
        PlaySoundParams {
            looped: true,
            volume: 1.,
        },
    );

    loop {
        let delta_time = get_frame_time();

        clear_background(BLACK);

        // Draw the starfield background via the shader material.
        material.set_uniform("iResolution", (screen_width(), screen_height()));
        material.set_uniform("direction_modifier", direction_modifier);
        gl_use_material(&material);
        draw_texture_ex(
            &render_target.texture,
            0.,
            0.,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(screen_width(), screen_height())),
                ..Default::default()
            },
        );
        gl_use_default_material();

        // --- State-driven game logic ---

        match state {
            GameState::Menu => {
                // Toggle fullscreen on F press.
                if is_key_pressed(KeyCode::F) {
                    fullscreen = !fullscreen;
                    set_fullscreen(fullscreen);
                }

                draw_menu(&score_history, &high_score, fullscreen);

                // Start the game — show tutorial on first launch, skip it otherwise.
                if is_key_pressed(KeyCode::Space) {
                    if first_time {
                        state = GameState::Tutorial;
                    } else {
                        state = GameState::Playing;
                    }
                }
            }
            GameState::Tutorial => {
                draw_tutorial();
                if is_key_pressed(KeyCode::Space) {
                    first_time = false;
                    state = GameState::Playing;
                }
            }
            GameState::Playing => {
                // --- Player movement ---
                // Horizontal movement also accumulates a direction modifier
                // that is passed to the starfield shader for visual effect.
                ship_sprite.set_animation(0);
                if is_key_down(KeyCode::Right) || is_key_down(KeyCode::D) {
                    player.x += player.speed * delta_time;
                    direction_modifier += 0.05 * delta_time;
                    ship_sprite.set_animation(2);
                }
                if is_key_down(KeyCode::Left) || is_key_down(KeyCode::A) {
                    player.x -= player.speed * delta_time;
                    direction_modifier -= 0.05 * delta_time;
                    ship_sprite.set_animation(1);
                }
                if is_key_down(KeyCode::Down) || is_key_down(KeyCode::S) {
                    player.y += player.speed * delta_time;
                }
                if is_key_down(KeyCode::Up) || is_key_down(KeyCode::W) {
                    player.y -= player.speed * delta_time;
                }

                // --- Shooting ---
                // Fire a bullet if space is held and the cooldown has expired.
                if is_key_down(KeyCode::Space) && shot_cooldown <= 0.0 {
                    bullets.push(Obstacle {
                        x: player.x,
                        y: player.y,
                        speed: player.speed * 2.0,
                        size: 20.0,
                        collided: false,
                    });
                    play_sound_once(&resources.sound_laser);
                    shot_cooldown = shot_cooldown_duration;
                    bolt_sprite.set_animation(0);
                }

                // Decrement cooldown each frame.
                shot_cooldown -= delta_time;

                // --- Keep player within screen bounds ---
                let half_size = player.size / 2.0;
                player.x = clamp(player.x, half_size, screen_width() - half_size);
                player.y = clamp(player.y, half_size, screen_height() - half_size);

                // --- Score accrual ---
                // The score increases continuously while alive, scaling difficulty over time.
                score += delta_time;

                // --- Obstacle spawning ---
                // Spawn chance increases with score, capped at a maximum to prevent
                // the screen from becoming completely unplayable.
                let spawn_chance = 5.0 + (score * 0.1).min(80.0);
                if rand::gen_range(0.0, 1000.0) < spawn_chance {
                    let size = rand::gen_range(18.0, 54.0);
                    let speed = rand::gen_range(150.0 + score, 270.0 + score * 0.6);
                    obstacles.push(Obstacle {
                        size,
                        speed,
                        x: rand::gen_range(size / 2.0, screen_width() - size / 2.0),
                        y: -size,
                        collided: false,
                    });
                }

                // --- Update positions ---
                // Obstacles fall downward; bullets travel upward.
                for obstacle in &mut obstacles {
                    obstacle.y += obstacle.speed * delta_time;
                }
                for bullet in &mut bullets {
                    bullet.y -= bullet.speed * delta_time;
                }
                ship_sprite.update(delta_time);
                enemy_small_sprite.update(delta_time);
                enemy_medium_sprite.update(delta_time);
                enemy_big_sprite.update(delta_time);
                bolt_sprite.update(delta_time);

                // Remove objects that have moved off-screen or have been marked as collided.
                obstacles.retain(|obstacle| obstacle.y < screen_height() + obstacle.size);
                bullets.retain(|bullet| bullet.y > -bullet.size);
                obstacles.retain(|obstacle| !obstacle.collided);
                bullets.retain(|bullet| !bullet.collided);

                // --- Collision: player vs obstacles ---
                // If any obstacle overlaps the player, end the game and update the high score.
                if obstacles.iter().any(|obstacle| obstacle.rect().overlaps(&player.rect())) {
                    if score > high_score {
                        high_score = score;
                        save_high_score(high_score);
                    }
                    state = GameState::GameOver;
                }

                // --- Rendering (gameplay layer) ---
                let ship_frame = ship_sprite.frame();
                draw_texture_ex(
                    &resources.ship_texture,
                    player.x - ship_frame.dest_size.x,
                    player.y - ship_frame.dest_size.y,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(ship_frame.dest_size * 2.0),
                        source: Some(ship_frame.source_rect),
                        ..Default::default()
                    },
                );

                // Draw each enemy with the sprite matching its size tier.
                // Obstacle sizes range from 18.0 to 54.0, split into three even bands.
                let small_frame = enemy_small_sprite.frame();
                let medium_frame = enemy_medium_sprite.frame();
                let big_frame = enemy_big_sprite.frame();
                for obstacle in &obstacles {
                    let (texture, frame) = if obstacle.size < 30.0 {
                        (&resources.enemy_small_texture, &small_frame)
                    } else if obstacle.size < 42.0 {
                        (&resources.enemy_medium_texture, &medium_frame)
                    } else {
                        (&resources.enemy_big_texture, &big_frame)
                    };

                    draw_texture_ex(
                        texture,
                        obstacle.x - obstacle.size / 2.0,
                        obstacle.y - obstacle.size / 2.0,
                        WHITE,
                        DrawTextureParams {
                            dest_size: Some(vec2(obstacle.size, obstacle.size)),
                            source: Some(frame.source_rect),
                            ..Default::default()
                        },
                    );
                }

                for (explosion, coords) in explosions.iter_mut() {
                    explosion.draw(*coords);
                }

                // Bullets are drawn using the Bolt.png sprite.
                let bolt_frame = bolt_sprite.frame();
                for bullet in &bullets {
                    draw_texture_ex(
                        &resources.bolt_texture,
                        bullet.x - bullet.size / 2.0,
                        bullet.y - bullet.size / 2.0,
                        WHITE,
                        DrawTextureParams {
                            dest_size: Some(vec2(bullet.size, bullet.size)),
                            source: Some(bolt_frame.source_rect),
                            ..Default::default()
                        },
                    );
                }

                // --- Collision: bullets vs obstacles ---
                // When a bullet hits an obstacle, both are marked as collided,
                // an explosion particle effect is spawned, and points are awarded.
                for obstacle in obstacles.iter_mut() {
                    for bullet in bullets.iter_mut() {
                        if bullet.collides_with(obstacle) {
                            bullet.collided = true;
                            obstacle.collided = true;
                            cubes_destroyed += 1;
                            score += obstacle.size.round() as f32;
                            high_score = high_score.max(score);
                            let explosion_texture = if obstacle.size < 30.0 {
                                resources.explosion_small_texture.clone()
                            } else if obstacle.size < 42.0 {
                                resources.explosion_medium_texture.clone()
                            } else {
                                resources.explosion_big_texture.clone()
                            };
                            explosions.push((
                                Emitter::new(EmitterConfig {
                                    amount: obstacle.size.round() as u32 * 2,
                                    ..particle_explosion(explosion_texture)
                                }),
                                vec2(obstacle.x, obstacle.y),
                            ));
                        play_sound_once(&resources.sound_explosion);
                        }
                    }
                }

                // --- HUD ---
                let score_text = format!("Score: {:.1}", score);
                draw_text(&score_text, 20.0, 30.0, 28.0, WHITE);
                let high_score_text = format!("High score: {:.1}", high_score);
                draw_text(&high_score_text, 20.0, 60.0, 24.0, LIGHTGRAY);
                let cubes_text = format!("Cubes shot: {}", cubes_destroyed);
                draw_text(&cubes_text, 20.0, 90.0, 24.0, LIGHTGRAY);
            }
            GameState::GameOver => {
                draw_gameover_screen(&score, &high_score);

                // Reset the game state and return to the menu on space press.
                if is_key_pressed(KeyCode::Space) {
                    score_history.push(score);
                    obstacles.clear();
                    bullets.clear();
                    player.x = screen_width() / 2.0;
                    player.y = screen_height() * 0.8;
                    score = 0.0;
                    explosions.clear();
                    cubes_destroyed = 0;
                    shot_cooldown = 0.0;
                    state = GameState::Menu;
                }
            }
        }

        next_frame().await;
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that the high-score path is correctly constructed relative to a given directory.
    #[test]
    fn high_score_path_is_relative_to_the_provided_directory() {
        let path = high_score_path_for(Path::new("/tmp/game"));
        assert_eq!(path, PathBuf::from("/tmp/game/Save/highscore.dat"));
    }

    /// Verifies that high scores can be saved to and loaded from disk.
    #[test]
    fn saves_and_loads_high_score() {
        let path = high_score_path();
        let _ = fs::remove_file(&path);

        save_high_score(42.5);
        assert_eq!(load_high_score(), 42.5);
        assert!(path.exists());

        let _ = fs::remove_file(&path);
    }
}