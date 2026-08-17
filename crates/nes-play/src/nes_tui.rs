//! Terminal frontend for `nes-play`.
//!
//! The terminal frontend owns lifecycle, input, timing, and presentation.

mod tui_renderer;

use tui_renderer::{Renderer, TextAtlas};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute, queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{
        self, Clear, ClearType, DisableLineWrap, EnableLineWrap, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use nes_core::{
    cartridge,
    console::Console,
    controller::{Button, ButtonState},
    ines, FrameOutput,
};
use std::{
    error::Error,
    fs::File,
    io::{self, Stdout, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / 60);
const HELP_LINE: &str = "WASD move  J/K A/B  ,/. Select/Start  Esc/Ctrl-C quit";

fn usage(program: &str) {
    eprintln!("usage: {program} ROM");
    eprintln!("       {program} --help");
}

fn load_console(path: &Path) -> Result<(Console, Option<TextAtlas>), Box<dyn Error>> {
    let mut file = File::open(path)
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))?;
    let (cartridge_image, mapper_number) = ines::load(&mut file).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not a readable iNES or UNIF ROM", path.display()),
        )
    })?;

    let text_atlas = TextAtlas::from_chr(cartridge_image.chr.get_banks());
    let console = if mapper_number == 0 {
        Console::new_nrom(cartridge_image)
    } else {
        let mapper = cartridge::new(cartridge_image, mapper_number).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported mapper {mapper_number} in {}", path.display()),
            )
        })?;
        Console::new(mapper)
    };
    Ok((console, text_atlas))
}

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut guard = Self { active: true };
        let mut stdout = io::stdout();
        if let Err(error) = execute!(
            stdout,
            EnterAlternateScreen,
            Hide,
            DisableLineWrap,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES),
            Clear(ClearType::All),
            MoveTo(0, 0)
        ) {
            guard.active = false;
            let _ = execute!(stdout, PopKeyboardEnhancementFlags);
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            ResetColor,
            Show,
            EnableLineWrap,
            PopKeyboardEnhancementFlags,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
        self.active = false;
    }
}

fn key_button(code: KeyCode) -> Option<Button> {
    match code {
        KeyCode::Char('w') | KeyCode::Char('W') => Some(Button::Up),
        KeyCode::Char('a') | KeyCode::Char('A') => Some(Button::Left),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(Button::Down),
        KeyCode::Char('d') | KeyCode::Char('D') => Some(Button::Right),
        KeyCode::Char('j') | KeyCode::Char('J') => Some(Button::B),
        KeyCode::Char('k') | KeyCode::Char('K') => Some(Button::A),
        KeyCode::Char(',') => Some(Button::Select),
        KeyCode::Char('.') => Some(Button::Start),
        _ => None,
    }
}

/// Drain all currently queued terminal events before emulating the next frame.
/// Crossterm's release events let the held state remain separate from the
/// event stream while still working with key-repeat from ordinary terminals.
fn drain_events(buttons: &mut ButtonState) -> io::Result<bool> {
    let mut quit = false;
    while event::poll(Duration::ZERO)? {
        let event = event::read()?;
        match event {
            Event::FocusLost => *buttons = ButtonState::default(),
            Event::Key(key) => {
                if key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    quit = true;
                    continue;
                }

                if let Some(button) = key_button(key.code) {
                    if key.kind == KeyEventKind::Release {
                        buttons.unset(button);
                    } else {
                        buttons.set(button);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(quit)
}

fn wait_for_frame(deadline: Instant, buttons: &mut ButtonState) -> io::Result<bool> {
    loop {
        if drain_events(buttons)? {
            return Ok(true);
        }

        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }

        // Poll in short slices so a terminal without release reporting still
        // feels responsive and a resize is reflected before the next frame.
        let remaining = deadline.saturating_duration_since(now);
        if event::poll(remaining.min(Duration::from_millis(5)))? {
            let event = event::read()?;
            match event {
                Event::FocusLost => *buttons = ButtonState::default(),
                Event::Key(key) => {
                    if key.code == KeyCode::Esc
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        return Ok(true);
                    }
                    if let Some(button) = key_button(key.code) {
                        if key.kind == KeyEventKind::Release {
                            buttons.unset(button);
                        } else {
                            buttons.set(button);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn run(
    mut console: Console,
    rom_name: &str,
    text_atlas: Option<TextAtlas>,
) -> Result<(), Box<dyn Error>> {
    let _terminal = TerminalGuard::enter()?;
    let mut stdout = io::stdout();
    let (columns, rows) = terminal::size()?;
    let mut renderer = Renderer::for_terminal(columns as usize, rows.saturating_sub(2) as usize);
    renderer.set_text_atlas(text_atlas);
    let mut buttons = ButtonState::default();
    let mut next_frame_at = Instant::now();
    let started_at = next_frame_at;
    let mut rendered_frames = 0u64;

    loop {
        if wait_for_frame(next_frame_at, &mut buttons)? {
            break;
        }
        if drain_events(&mut buttons)? {
            break;
        }

        console.update_buttons(buttons);
        let frame = console.next_frame();
        rendered_frames += 1;
        let elapsed = started_at.elapsed().as_secs_f64();
        let fps = rendered_frames as f64 / elapsed.max(f64::EPSILON);
        draw(&mut stdout, &mut renderer, &frame, rom_name, buttons, fps)?;

        next_frame_at += FRAME_DURATION;
        let now = Instant::now();
        if next_frame_at + FRAME_DURATION * 4 < now {
            next_frame_at = now + FRAME_DURATION;
        }
    }
    Ok(())
}

fn draw(
    stdout: &mut Stdout,
    renderer: &mut Renderer,
    frame: &FrameOutput<'_>,
    rom_name: &str,
    buttons: ButtonState,
    fps: f64,
) -> io::Result<()> {
    let (columns, rows) = terminal::size()?;
    let image_rows = rows.saturating_sub(2).max(1);
    let config = renderer.config();
    if config.grid.columns != columns as usize || config.grid.rows != image_rows as usize {
        renderer.resize(columns as usize, image_rows as usize);
    }
    let rendered = renderer.render_frame(frame);
    queue!(
        stdout,
        MoveTo(0, 0),
        SetAttribute(Attribute::NormalIntensity)
    )?;
    let mut current_color = None;
    for (y, row) in rendered.rows().enumerate() {
        queue!(stdout, MoveTo(0, y as u16), Clear(ClearType::CurrentLine))?;
        let mut bold = false;
        for cell in row {
            let color = cell_color(cell);
            if current_color != Some(color) {
                queue!(stdout, SetForegroundColor(color))?;
                current_color = Some(color);
            }
            if cell.role == tui_renderer::GlyphRole::Text && !bold {
                queue!(stdout, SetAttribute(Attribute::Bold))?;
                bold = true;
            } else if cell.role != tui_renderer::GlyphRole::Text && bold {
                queue!(stdout, SetAttribute(Attribute::NormalIntensity))?;
                bold = false;
            }
            queue!(stdout, Print(cell.glyph))?;
        }
        if bold {
            queue!(stdout, SetAttribute(Attribute::NormalIntensity))?;
        }
    }

    let status_y = rows.saturating_sub(2);
    let status = format!(
        " ROM {rom_name} | frame {:>8} | {:>5.1} fps | {}x{} | buttons 0x{:02x}",
        frame.frame_number,
        fps,
        rendered.grid.content_columns,
        rendered.grid.content_rows,
        buttons.bits()
    );
    queue!(
        stdout,
        SetForegroundColor(Color::AnsiValue(252)),
        SetAttribute(Attribute::NormalIntensity),
        MoveTo(0, status_y),
        Clear(ClearType::CurrentLine),
        Print(status),
        MoveTo(0, rows.saturating_sub(1)),
        Clear(ClearType::CurrentLine),
        Print(HELP_LINE),
        ResetColor
    )?;
    stdout.flush()
}

fn cell_color(cell: &tui_renderer::RenderedCell) -> Color {
    match cell.role {
        tui_renderer::GlyphRole::Text => Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        tui_renderer::GlyphRole::Sprite => Color::Rgb {
            r: 255,
            g: 204,
            b: 112,
        },
        tui_renderer::GlyphRole::Detail => Color::Rgb {
            r: 198,
            g: 204,
            b: 220,
        },
        tui_renderer::GlyphRole::Edge | tui_renderer::GlyphRole::Corner => Color::Rgb {
            r: 164,
            g: 176,
            b: 196,
        },
        tui_renderer::GlyphRole::Fill => Color::Rgb {
            r: 124,
            g: 134,
            b: 154,
        },
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os();
    let program = arguments
        .next()
        .unwrap_or_else(|| "nes-tui".into())
        .to_string_lossy()
        .into_owned();
    let Some(argument) = arguments.next() else {
        usage(&program);
        return Ok(());
    };
    if argument == "--help" || argument == "-h" {
        usage(&program);
        return Ok(());
    }
    if arguments.next().is_some() {
        usage(&program);
        return Err("expected exactly one ROM path".into());
    }

    let rom_path = PathBuf::from(argument);
    let rom_name = rom_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("rom")
        .to_owned();
    let (console, text_atlas) = load_console(&rom_path)?;
    run(console, &rom_name, text_atlas)
}
