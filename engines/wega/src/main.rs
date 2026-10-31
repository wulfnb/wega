//! One private worker per Wega tab. No window, network control port or Servo VM.
use std::{
    cell::RefCell,
    collections::VecDeque,
    io::Write,
    rc::Rc,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, PageEvent, Pointer, PointerKind, Url};
use wega_protocol as protocol;
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
    let (version, url, policy, storage_dir, broker_address, broker_token, frame_path) =
        match receiver.recv()? {
            Work::Command(Command::Init {
                version,
                url,
                offline,
                blocked_hosts,
                allowed_hosts,
                storage_dir,
                broker_address,
                broker_token,
                frame_path,
                // v5 only adds the requested PageState exchange. Existing v4
                // commands/events keep their exact shape, allowing a running older
                // browser to finish its session after an atomic worker update.
            }) if version == protocol::VERSION || version == 4 => (
                version,
                Url::parse(&url)?,
                NetworkPolicy {
                    offline,
                    blocked_hosts,
                    allowed_hosts,
                },
                storage_dir,
                broker_address,
                broker_token,
                frame_path,
            ),
            _ => return Err("Expected versioned Init as the first private command".into()),
        };
    let wake = sender.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = wake.try_send(Work::Wake);
    });
    let mut engine = if let (Some(address), Some(token)) = (broker_address, broker_token) {
        Engine::brokered(Limits::default(), policy, wake, &address, token)?
    } else {
        Engine::new(Limits::default(), policy, wake)?
    };
    if let Some(path) = storage_dir {
        engine = engine.with_storage_dir(path.into());
    }
    let mut view = engine.create_view(url)?;
    let shared = frame_path.is_some();
    if let Some(path) = frame_path {
        view.configure_shared_frames(std::path::Path::new(&path))?;
    }
    let mut credits = [None, None];
    let mut sequence = 0_u64;
    let mut pending_target = None;
    let mut serial = 0_u64;
    let mut frozen = false;
    let interval = std::time::Duration::from_secs(2);
    let mut sampled = std::time::Instant::now() - interval;
    let mut rss_bytes = wega_platform::resident_bytes();
    let mut metrics = (0, 0);
    let output = Rc::new(RefCell::new(std::io::stdout().lock()));
    let receiver = Rc::new(receiver);
    // Commands that arrive while a dialog waits for its answer run afterwards.
    let deferred: Rc<RefCell<VecDeque<Work>>> = Default::default();
    {
        let output = output.clone();
        let receiver = receiver.clone();
        let deferred = deferred.clone();
        view.set_dialog_handler(Box::new(move |kind, message, default| {
            let event = Event::Dialog {
                kind: kind.into(),
                message: message.into(),
                default: default.into(),
            };
            if protocol::write(&mut *output.borrow_mut(), &event).is_err() {
                return None;
            }
            let deadline = Instant::now() + Duration::from_secs(300);
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match receiver.recv_timeout(remaining) {
                    Ok(Work::Command(Command::DialogReply(value))) => return value,
                    Ok(Work::Wake) => {}
                    Ok(work @ Work::Command(Command::Shutdown)) => {
                        deferred.borrow_mut().push_back(work);
                        return None;
                    }
                    Ok(work) => {
                        let mut queue = deferred.borrow_mut();
                        if queue.len() < 64 {
                            queue.push_back(work);
                        }
                    }
                    Err(_) => return None,
                }
            }
        }));
    }
    let write = |event: &Event| protocol::write(&mut *output.borrow_mut(), event);
    write(&Event::Ready { version })?;
    let mut viewport = (1000, 700, 1.0, 1.0, false);
    let mut state = String::new();
    let frame_interval = std::time::Duration::from_millis(33);
    let mut last_frame = std::time::Instant::now() - std::time::Duration::from_secs(1);
    loop {
        if sampled.elapsed() >= interval {
            rss_bytes = wega_platform::resident_bytes();
            metrics = (view.node_count(), view.heap_bytes());
            sampled = std::time::Instant::now();
        }
        if let Some(result) = view.take_painted() {
            let completed_target = pending_target.take();
            match result {
                Ok(frame) => {
                    if frame.serial == serial && !frozen {
                        match frame.pixels {
                            wega_engine::Pixels::Shared { slot, sequence } => {
                                write(&Event::SharedFrame {
                                    serial: frame.serial,
                                    width: frame.width,
                                    height: frame.height,
                                    slot,
                                    sequence,
                                })?
                            }
                            wega_engine::Pixels::Owned(bytes) => {
                                write(&Event::Frame {
                                    serial: frame.serial,
                                    width: frame.width,
                                    height: frame.height,
                                })?;
                                let mut output = output.borrow_mut();
                                output.write_all(&bytes)?;
                                output.flush()?;
                            }
                        }
                    } else if let wega_engine::Pixels::Shared { slot, sequence } = frame.pixels
                        && credits[slot as usize] == Some(sequence)
                    {
                        credits[slot as usize] = None;
                    }
                }
                Err(error) => {
                    if let Some((slot, _)) = completed_target {
                        credits[slot as usize] = None;
                    }
                    write(&Event::Compatibility {
                        serial,
                        reason: error,
                    })?;
                }
            }
        }
        view.poll();
        let snapshot = Event::State {
            serial,
            rss_bytes,
            url: view.url.to_string(),
            title: view.title(),
            loading: view.loading,
            nodes: metrics.0,
            js_heap: metrics.1,
            back: view.document_history().0,
            forward: view.document_history().1,
        };
        let serialized = serde_json::to_string(&snapshot)?;
        if state != serialized {
            write(&snapshot)?;
            state = serialized;
        }
        while let Some(notice) = view.notices.pop_front() {
            write(&Event::Notice(notice.chars().take(4096).collect()))?;
        }
        if let Some(reason) = view.compatibility.take() {
            write(&Event::Compatibility {
                serial,
                reason: reason.chars().take(4096).collect(),
            })?;
        }
        while let Some(url) = view.next_navigation() {
            write(&Event::Navigate(url.to_string()))?;
        }
        while let Some(event) = view.events.pop_front() {
            write(&match event {
                PageEvent::Download(url) => Event::Download {
                    cookies: view.download_cookies(&url),
                    url: url.to_string(),
                },
                PageEvent::OpenWindow(url) => Event::OpenWindow(url.to_string()),
                PageEvent::Traverse(delta) => Event::TraverseUnhandled(delta),
                PageEvent::Cursor(cursor) => Event::Cursor(cursor),
            })?;
        }
        let slot = credits.iter().position(Option::is_none);
        let can_paint = !view.paint_pending() && (!shared || slot.is_some());
        if view.needs_paint() && can_paint && last_frame.elapsed() >= frame_interval {
            let target = if shared {
                let slot = slot.unwrap();
                sequence = sequence.wrapping_add(1);
                credits[slot] = Some(sequence);
                Some((slot as u8, sequence))
            } else {
                None
            };
            match view.begin_paint(
                wega_engine::Viewport {
                    window_size: (viewport.0, viewport.1),
                    hidpi_scale: viewport.2,
                    zoom: viewport.3,
                    color_scheme: if viewport.4 {
                        wega_engine::ColorScheme::Dark
                    } else {
                        wega_engine::ColorScheme::Light
                    },
                },
                serial,
                target,
            ) {
                Ok(()) => {
                    pending_target = target;
                    last_frame = std::time::Instant::now();
                }
                Err(error) => {
                    if let Some((slot, _)) = target {
                        credits[slot as usize] = None;
                    }
                    frozen = true;
                    view.freeze(true);
                    write(&Event::Compatibility {
                        serial,
                        reason: error,
                    })?;
                }
            }
        }
        let mut delay = view.next_wakeup();
        if view.needs_paint() && can_paint {
            let frame_delay = frame_interval.saturating_sub(last_frame.elapsed());
            delay = Some(delay.map_or(frame_delay, |d| d.min(frame_delay)));
        }
        let resource_delay = interval.saturating_sub(sampled.elapsed());
        delay = Some(delay.map_or(resource_delay, |delay| delay.min(resource_delay)));
        let queued = deferred.borrow_mut().pop_front();
        let work = if let Some(work) = queued {
            work
        } else if let Some(delay) = delay {
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
                Command::CheckPageState(request) => {
                    let dirty =
                        view.evaluate_json("__wega_page_has_edits()").as_deref() != Ok("false");
                    write(&Event::PageState { request, dirty })?;
                }
                Command::Load {
                    url,
                    serial: requested,
                } => {
                    serial = requested;
                    if let Err(error) = Url::parse(&url)
                        .map_err(|e| e.to_string())
                        .and_then(|url| view.load(url))
                    {
                        write(&Event::Notice(error))?;
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
                    view.request_redraw();
                }
                Command::Click { x, y } => view.click(x, y),
                Command::Pointer {
                    kind,
                    x,
                    y,
                    button,
                    buttons,
                    modifiers,
                    delta_x,
                    delta_y,
                } => {
                    let kind = match kind.as_str() {
                        "move" => Some(PointerKind::Move),
                        "down" => Some(PointerKind::Down),
                        "up" => Some(PointerKind::Up),
                        "leave" => Some(PointerKind::Leave),
                        "wheel" => Some(PointerKind::Wheel),
                        _ => None,
                    };
                    if let Some(kind) = kind
                        && x.is_finite()
                        && y.is_finite()
                        && delta_x.is_finite()
                        && delta_y.is_finite()
                    {
                        view.pointer(Pointer {
                            kind,
                            x,
                            y,
                            button,
                            buttons,
                            modifiers,
                            delta_x,
                            delta_y,
                        });
                    }
                }
                Command::Traverse(delta) => view.traverse(delta),
                // A late answer after the dialog timed out.
                Command::DialogReply(_) => {}
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
                Command::Scroll {
                    x,
                    y,
                    pointer_x,
                    pointer_y,
                } => view.scroll_at(x, y, pointer_x, pointer_y),
                Command::FrameConsumed { slot, sequence } => {
                    if let Some(credit) = credits.get_mut(slot as usize)
                        && *credit == Some(sequence)
                    {
                        *credit = None;
                    }
                }
                Command::Freeze(value) => {
                    frozen = value;
                    view.freeze(value);
                    sampled = std::time::Instant::now() - interval;
                }
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
                    // Apply live permissions without discarding the current DOM/input state.
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
                    write(&event)?;
                }
                Command::Init { .. } => return Err("Worker was initialized more than once".into()),
            },
        }
    }
    Ok(())
}
