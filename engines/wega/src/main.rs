//! One private worker per Wega tab. No window, network control port or Servo VM.
use protocol::{
    Command, ContentEvent, DialogId, DocumentContext, DocumentId, DocumentRevision, ErrorCode,
    Event, ProtocolError,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io::Write,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, PageEvent, Pointer, PointerKind, Url};
use wega_protocol as protocol;
enum Work {
    Command(Command),
    Rejected(protocol::RequestContext, ProtocolError),
    Wake,
}

fn document_context(view: &wega_engine::View, serial: u64, version: u32) -> DocumentContext {
    DocumentContext {
        document: DocumentId(if version >= 9 {
            view.document_generation()
        } else {
            serial
        }),
        revision: DocumentRevision(view.document_revision()),
    }
}

struct RunningRequest {
    queue: Arc<Mutex<protocol::queue::CommandQueue>>,
    context: protocol::RequestContext,
}

impl Drop for RunningRequest {
    fn drop(&mut self) {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finish(self.context);
    }
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
    let queue: Arc<Mutex<protocol::queue::CommandQueue>> = Default::default();
    let input_queue = queue.clone();
    let reader = sender.clone();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            match protocol::read::<Command>(&mut stdin) {
                Ok(Some(command)) => {
                    if let Command::CancelRequest(context) = command {
                        input_queue
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .cancel(context);
                        continue;
                    }
                    if let Some(context) = protocol::scoped_context(&command) {
                        let timeout = if matches!(&command, Command::CheckDocumentState(_)) {
                            protocol::PAGE_STATE_TIMEOUT_MS
                        } else {
                            protocol::EVALUATION_TIMEOUT_MS
                        };
                        let admitted = input_queue
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .admit(context, Instant::now(), Duration::from_millis(timeout));
                        if let Err(error) = admitted {
                            if reader.send(Work::Rejected(context, error)).is_err() {
                                break;
                            }
                            continue;
                        }
                    }
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
                // Legacy v4/v5 packets retain their shape. Scoped v6 operations
                // are only sent in response to the corresponding new commands.
            }) if protocol::supports_version(version) => (
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
    let mut pending_revision = None;
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
        let admission = queue.clone();
        let mut next_dialog = 0_u64;
        view.set_document_dialog_handler(Box::new(
            move |generation, revision, kind, message, default| {
                next_dialog = next_dialog.checked_add(1)?;
                let id = DialogId(next_dialog);
                let context = DocumentContext {
                    document: DocumentId(generation),
                    revision: DocumentRevision(revision),
                };
                // Parsing may reach an inline alert before poll() returns to the host loop.
                if version >= 9
                    && protocol::write(
                        &mut *output.borrow_mut(),
                        &Event::DocumentCommitted {
                            serial: generation,
                            revision: context.revision,
                        },
                    )
                    .is_err()
                {
                    return None;
                }
                let event = protocol::content_event(
                    version,
                    context,
                    ContentEvent::Dialog {
                        id,
                        kind: kind.into(),
                        message: message.into(),
                        default: default.into(),
                    },
                );
                if protocol::write(&mut *output.borrow_mut(), &event).is_err() {
                    return None;
                }
                let deadline = Instant::now() + Duration::from_secs(300);
                let answer = (|| {
                    loop {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        match receiver.recv_timeout(remaining) {
                            Ok(Work::Command(Command::AnswerDialog {
                                context: answered,
                                id: answered_id,
                                value,
                            })) if version >= 9 && answered == context && answered_id == id => {
                                return value;
                            }
                            Ok(Work::Command(Command::AnswerDialog { .. })) => {}
                            Ok(Work::Command(Command::DialogReply(value))) if version < 9 => {
                                return value;
                            }
                            Ok(Work::Command(Command::DialogReply(_))) => {}
                            Ok(Work::Wake) => {}
                            Ok(work @ Work::Command(Command::Shutdown)) => {
                                deferred.borrow_mut().push_back(work);
                                return None;
                            }
                            Ok(work @ Work::Command(Command::Load { .. } | Command::Stop)) => {
                                deferred.borrow_mut().push_back(work);
                                return None;
                            }
                            Ok(work) => {
                                let mut queue = deferred.borrow_mut();
                                if queue.len() < 64 {
                                    queue.push_back(work);
                                } else {
                                    let rejection = match work {
                                        Work::Command(command) => {
                                            protocol::scoped_context(&command).map(|context| {
                                                admission
                                                    .lock()
                                                    .unwrap_or_else(
                                                        std::sync::PoisonError::into_inner,
                                                    )
                                                    .finish(context);
                                                Event::RequestFailed {
                                                    context,
                                                    error: ProtocolError::new(
                                                        ErrorCode::ResourceLimit,
                                                        "Dialog command queue is full",
                                                    ),
                                                }
                                            })
                                        }
                                        Work::Rejected(context, error) => {
                                            Some(Event::RequestFailed { context, error })
                                        }
                                        Work::Wake => None,
                                    };
                                    if let Some(event) = rejection
                                        && protocol::write(&mut *output.borrow_mut(), &event)
                                            .is_err()
                                    {
                                        return None;
                                    }
                                }
                            }
                            Err(_) => return None,
                        }
                    }
                })();
                if version >= 9 {
                    let _ = protocol::write(
                        &mut *output.borrow_mut(),
                        &protocol::content_event(version, context, ContentEvent::DialogClosed(id)),
                    );
                }
                answer
            },
        ));
    }
    let write = |event: &Event| protocol::write(&mut *output.borrow_mut(), event);
    let mut reported_commit = None;
    let mut report_commit = |view: &wega_engine::View, serial| -> std::io::Result<()> {
        let context = document_context(view, serial, version);
        let serial = context.document.0;
        let revision = context.revision;
        if (version < 9 || revision.0 > 0) && reported_commit != Some((serial, revision)) {
            if version >= 7 {
                write(&Event::DocumentCommitted { serial, revision })?;
            }
            reported_commit = Some((serial, revision));
        }
        Ok(())
    };
    write(&Event::Ready { version })?;
    let mut viewport = (1000, 700, 1.0, 1.0, false);
    let mut state = String::new();
    let frame_interval = std::time::Duration::from_millis(33);
    let mut last_frame = std::time::Instant::now() - std::time::Duration::from_secs(1);
    loop {
        report_commit(&view, serial)?;
        if sampled.elapsed() >= interval {
            rss_bytes = wega_platform::resident_bytes();
            metrics = (view.node_count(), view.heap_bytes());
            sampled = std::time::Instant::now();
        }
        if let Some(result) = view.take_painted() {
            let completed_target = pending_target.take();
            let completed_revision = pending_revision.take().unwrap_or(DocumentRevision(0));
            match result {
                Ok(frame) => {
                    if frame.serial == serial
                        && completed_revision.0 == view.document_revision()
                        && !frozen
                    {
                        match frame.pixels {
                            wega_engine::Pixels::Shared { slot, sequence } => {
                                write(&Event::SharedFrame {
                                    serial: frame.serial,
                                    revision: completed_revision,
                                    width: frame.width,
                                    height: frame.height,
                                    slot,
                                    sequence,
                                })?
                            }
                            wega_engine::Pixels::Owned(bytes) => {
                                write(&Event::Frame {
                                    serial: frame.serial,
                                    revision: completed_revision,
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
                    if completed_revision.0 == view.document_revision() {
                        write(&protocol::content_event(
                            version,
                            document_context(&view, serial, version),
                            ContentEvent::Compatibility(error),
                        ))?;
                    }
                }
            }
        }
        view.poll();
        report_commit(&view, serial)?;
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
            write(&protocol::content_event(
                version,
                document_context(&view, serial, version),
                ContentEvent::Notice(notice.chars().take(4096).collect()),
            ))?;
        }
        if let Some(reason) = view.compatibility.take() {
            write(&protocol::content_event(
                version,
                document_context(&view, serial, version),
                ContentEvent::Compatibility(reason.chars().take(4096).collect()),
            ))?;
        }
        while let Some(url) = view.next_navigation() {
            write(&protocol::content_event(
                version,
                document_context(&view, serial, version),
                ContentEvent::Navigate(url.to_string()),
            ))?;
        }
        while let Some(event) = view.events.pop_front() {
            let event = match event {
                PageEvent::Download(url) => ContentEvent::Download {
                    cookies: view.download_cookies(&url),
                    url: url.to_string(),
                },
                PageEvent::OpenWindow(url) => ContentEvent::OpenWindow(url.to_string()),
                PageEvent::Traverse(delta) => ContentEvent::TraverseUnhandled(delta),
                PageEvent::Cursor(cursor) => ContentEvent::Cursor(cursor),
            };
            write(&protocol::content_event(
                version,
                document_context(&view, serial, version),
                event,
            ))?;
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
                    pending_revision = Some(DocumentRevision(view.document_revision()));
                    last_frame = std::time::Instant::now();
                }
                Err(error) => {
                    if let Some((slot, _)) = target {
                        credits[slot as usize] = None;
                    }
                    frozen = true;
                    view.freeze(true);
                    write(&protocol::content_event(
                        version,
                        document_context(&view, serial, version),
                        ContentEvent::Compatibility(error),
                    ))?;
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
        let context = match &work {
            Work::Command(command) => protocol::scoped_context(command),
            _ => None,
        };
        let _running = if let Some(context) = context {
            let started = queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .start(context, Instant::now());
            if let Err(error) = started {
                write(&Event::RequestFailed { context, error })?;
                continue;
            }
            Some(RunningRequest {
                queue: queue.clone(),
                context,
            })
        } else {
            None
        };
        match work {
            Work::Wake => {}
            Work::Rejected(context, error) => write(&Event::RequestFailed { context, error })?,
            Work::Command(command) => match command {
                Command::CancelRequest(_) => {}    // Consumed by the input thread.
                Command::AnswerDialog { .. } => {} // A late answer has no authority over another prompt.
                Command::Capabilities => {
                    write(&Event::Capabilities {
                        version,
                        supported: protocol::capabilities(version),
                    })?;
                }
                Command::EvaluateDocument { context, source } => {
                    if context.document != DocumentId(serial)
                        || (version >= 7 && context.revision.0 != view.document_revision())
                    {
                        write(&Event::RequestFailed {
                            context,
                            error: ProtocolError::new(
                                ErrorCode::StaleDocument,
                                "Document navigated before evaluation",
                            ),
                        })?;
                        continue;
                    }
                    // Navigation can retain the old DOM until the replacement commits.
                    // Never execute a new-generation request against that retained DOM.
                    if view.loading {
                        write(&Event::RequestFailed {
                            context,
                            error: ProtocolError::new(
                                ErrorCode::InvalidRequest,
                                "Document has not finished loading",
                            ),
                        })?;
                        continue;
                    }
                    let result = match view.evaluate_json(&source) {
                        Ok(value) if value.len() <= protocol::MAX_RESULT_BYTES => Ok(value),
                        Ok(_) => Err(ProtocolError::new(
                            ErrorCode::ResourceLimit,
                            "Evaluation result exceeds the IPC budget",
                        )),
                        Err(error) => Err(ProtocolError::new(ErrorCode::ScriptException, error)),
                    };
                    write(&Event::DocumentEvaluated { context, result })?;
                }
                Command::CheckDocumentState(context) => {
                    if context.document != DocumentId(serial)
                        || (version >= 7 && context.revision.0 != view.document_revision())
                    {
                        write(&Event::RequestFailed {
                            context,
                            error: ProtocolError::new(
                                ErrorCode::StaleDocument,
                                "Document navigated before state check",
                            ),
                        })?;
                        continue;
                    }
                    if view.loading {
                        write(&Event::RequestFailed {
                            context,
                            error: ProtocolError::new(
                                ErrorCode::InvalidRequest,
                                "Document has not finished loading",
                            ),
                        })?;
                        continue;
                    }
                    let dirty =
                        view.evaluate_json("__wega_page_has_edits()").as_deref() != Ok("false");
                    write(&Event::DocumentState { context, dirty })?;
                }
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
                        .and_then(|url| view.load_for_navigation(url, requested))
                    {
                        view.stop_for_navigation(requested);
                        report_commit(&view, serial)?;
                        write(&protocol::content_event(
                            version,
                            document_context(&view, serial, version),
                            ContentEvent::Notice(error),
                        ))?;
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
                Command::Init { .. } => {
                    return Err("Worker was initialized more than once".into());
                }
            },
        }
    }
    drop(view);
    drop(engine);
    if version >= 10 {
        write(&Event::ShutdownComplete)?;
    }
    Ok(())
}
