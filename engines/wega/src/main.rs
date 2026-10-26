//! One private worker per Wega tab. No window, network control port or Servo VM.
use std::{
    io::Write,
    sync::{Arc, mpsc},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url};
#[path = "../../../src/engine_protocol.rs"]
mod protocol;
use protocol::{Command, Event};
enum Work {
    Command(Command),
    Wake,
}

fn main() {
    // Keep enough native stack headroom on Windows for the bounded V8 host.
    let result = std::thread::Builder::new()
        .name("wega-dom".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| run().map_err(|error| error.to_string()))
        .map_err(|error| error.to_string())
        .and_then(|thread| {
            thread
                .join()
                .unwrap_or_else(|_| Err("DOM worker thread panicked".into()))
        });
    if let Err(error) = result {
        eprintln!("Wega engine worker: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--lite") {
        deno_core::v8_set_flags(vec![String::new(), "--lite-mode".into()]);
    }
    let (sender, receiver) = mpsc::sync_channel(32);
    let reader = sender.clone();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            match protocol::read::<Command>(&mut stdin) {
                Ok(Some(command)) => {
                    if reader.send(Work::Command(command)).is_err() {
                        break;
                    }
                }
                _ => {
                    let _ = reader.send(Work::Command(Command::Shutdown));
                    break;
                }
            }
        }
    });
    let (url, policy, storage_dir) = match receiver.recv()? {
        Work::Command(Command::Init {
            version,
            url,
            offline,
            blocked_hosts,
            allowed_hosts,
            storage_dir,
        }) if version == protocol::VERSION => (
            Url::parse(&url)?,
            NetworkPolicy {
                offline,
                blocked_hosts,
                allowed_hosts,
            },
            storage_dir,
        ),
        _ => return Err("Expected versioned Init as the first private command".into()),
    };
    let wake = sender.clone();
    let mut engine = Engine::new(
        Limits::default(),
        policy,
        Arc::new(move || {
            let _ = wake.try_send(Work::Wake);
        }),
    )?;
    if let Some(path) = storage_dir {
        engine = engine.with_storage_dir(path.into());
    }
    let mut view = engine.create_view(url)?;
    let mut output = std::io::stdout().lock();
    protocol::write(
        &mut output,
        &Event::Ready {
            version: protocol::VERSION,
        },
    )?;
    let mut viewport = (1000, 700, 1.0, 1.0, false);
    let mut state = String::new();
    let frame_interval = std::time::Duration::from_millis(33);
    let mut last_frame = std::time::Instant::now() - std::time::Duration::from_secs(1);
    loop {
        view.poll();
        let snapshot = Event::State {
            url: view.url.to_string(),
            title: view.title(),
            loading: view.loading,
            nodes: view.node_count(),
            js_heap: view.heap_bytes(),
        };
        let serialized = serde_json::to_string(&snapshot)?;
        if state != serialized {
            protocol::write(&mut output, &snapshot)?;
            state = serialized;
        }
        while let Some(notice) = view.notices.pop_front() {
            protocol::write(
                &mut output,
                &Event::Notice(notice.chars().take(4096).collect()),
            )?;
        }
        while let Some(url) = view.next_navigation() {
            protocol::write(&mut output, &Event::Navigate(url.to_string()))?;
        }
        if view.needs_paint() && last_frame.elapsed() >= frame_interval {
            match view.paint(viewport.0, viewport.1, viewport.2, viewport.3, viewport.4) {
                Ok(frame) => {
                    protocol::write(
                        &mut output,
                        &Event::Frame {
                            width: frame.width,
                            height: frame.height,
                        },
                    )?;
                    output.write_all(&frame.rgba)?;
                    output.flush()?;
                    last_frame = std::time::Instant::now();
                }
                Err(error) => {
                    view.freeze(true);
                    protocol::write(&mut output, &Event::Notice(error))?;
                }
            }
        }
        let mut delay = view.next_wakeup();
        if view.needs_paint() {
            let frame_delay = frame_interval.saturating_sub(last_frame.elapsed());
            delay = Some(delay.map_or(frame_delay, |d| d.min(frame_delay)));
        }
        let work = if let Some(delay) = delay {
            match receiver.recv_timeout(delay) {
                Ok(work) => work,
                Err(mpsc::RecvTimeoutError::Timeout) => Work::Wake,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match receiver.recv() {
                Ok(work) => work,
                Err(_) => break,
            }
        };
        match work {
            Work::Wake => {}
            Work::Command(command) => match command {
                Command::Shutdown => break,
                Command::Load(url) => {
                    if let Err(error) = Url::parse(&url)
                        .map_err(|e| e.to_string())
                        .and_then(|url| view.load(url))
                    {
                        protocol::write(&mut output, &Event::Notice(error))?;
                    }
                }
                Command::Resize {
                    width,
                    height,
                    scale,
                    zoom,
                    dark,
                } => {
                    protocol::frame_bytes(width, height)?;
                    viewport = (width, height, scale, zoom, dark);
                    view.freeze(false);
                }
                Command::Click { x, y } => view.click(x, y),
                Command::Key {
                    key,
                    code,
                    text,
                    pressed,
                    modifiers,
                    repeat,
                } => {
                    use keyboard_types::{Code, Key, Location, Modifiers};
                    use wega_engine::events::{BlitzKeyEvent, KeyState};
                    let mut flags = Modifiers::empty();
                    for (bit, flag) in [
                        (1, Modifiers::SHIFT),
                        (2, Modifiers::CONTROL),
                        (4, Modifiers::ALT),
                        (8, Modifiers::META),
                    ] {
                        if modifiers & bit != 0 {
                            flags.insert(flag);
                        }
                    }
                    view.key(BlitzKeyEvent {
                        key: key.parse().unwrap_or(Key::Character(key)),
                        code: code.parse().unwrap_or(Code::Unidentified),
                        text: text.map(Into::into),
                        state: if pressed {
                            KeyState::Pressed
                        } else {
                            KeyState::Released
                        },
                        location: Location::Standard,
                        modifiers: flags,
                        is_auto_repeating: repeat,
                        is_composing: false,
                    });
                }
                Command::Ime {
                    text,
                    preedit,
                    cursor,
                } => view.ime(if preedit {
                    wega_engine::events::BlitzImeEvent::Preedit(text, cursor)
                } else {
                    wega_engine::events::BlitzImeEvent::Commit(text)
                }),
                Command::Scroll { x, y } => view.scroll(x, y),
                Command::Freeze(frozen) => view.freeze(frozen),
                Command::Stop => view.stop(),
                Command::Redraw => view.request_redraw(),
                Command::Policy {
                    offline,
                    blocked_hosts,
                    allowed_hosts,
                } => {
                    engine.set_policy(NetworkPolicy {
                        offline,
                        blocked_hosts,
                        allowed_hosts,
                    });
                    if let Err(error) = view.load(view.url.clone()) {
                        protocol::write(&mut output, &Event::Notice(error))?;
                    }
                }
                Command::Evaluate(source) => {
                    let result = view.evaluate_json(&source);
                    let event = match result {
                        Ok(value) => Event::Evaluated { value, error: None },
                        Err(error) => Event::Evaluated {
                            value: String::new(),
                            error: Some(error),
                        },
                    };
                    protocol::write(&mut output, &event)?;
                }
                Command::Init { .. } => return Err("Worker was initialized more than once".into()),
            },
        }
    }
    Ok(())
}
