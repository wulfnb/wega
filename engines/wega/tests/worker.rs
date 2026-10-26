//! Real subprocess protocol: two frame credits, exact acknowledgements and freeze.
#[path = "../../../src/engine_protocol.rs"]
mod protocol;
use protocol::{Command, Event};
use std::{
    process::{Child, ChildStdin, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
struct Worker {
    child: Child,
    input: ChildStdin,
    events: mpsc::Receiver<Event>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Worker {
    fn start(path: &std::path::Path) -> Self {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_wega-engine"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(Some(event)) = protocol::read::<Event>(&mut output) {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });
        let mut worker = Self {
            child,
            input,
            events,
        };
        worker.send(Command::Init {
            version: protocol::VERSION,
            url: "about:blank".into(),
            offline: true,
            blocked_hosts: String::new(),
            allowed_hosts: String::new(),
            storage_dir: None,
            broker_address: None,
            broker_token: None,
            frame_path: Some(path.to_string_lossy().into_owned()),
        });
        worker.until(|event| {
            matches!(
                event,
                Event::Ready {
                    version: protocol::VERSION
                }
            )
        });
        worker
    }
    fn send(&mut self, command: Command) {
        protocol::write(&mut self.input, &command).unwrap();
    }
    fn until(&self, test: impl Fn(&Event) -> bool) -> Event {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let event = self
                .events
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .expect("worker event deadline");
            if test(&event) {
                return event;
            }
        }
    }
    fn frame(&self) -> (u8, u64) {
        match self.until(|event| matches!(event, Event::SharedFrame { .. })) {
            Event::SharedFrame {
                slot,
                sequence,
                width,
                height,
                ..
            } => {
                protocol::frame_bytes(width, height).unwrap();
                (slot, sequence)
            }
            _ => unreachable!(),
        }
    }
    fn no_frame(&self) {
        let until = Instant::now() + Duration::from_millis(150);
        while let Ok(event) = self
            .events
            .recv_timeout(until.saturating_duration_since(Instant::now()))
        {
            assert!(
                !matches!(event, Event::SharedFrame { .. }),
                "unexpected frame: {event:?}"
            );
            if Instant::now() >= until {
                break;
            }
        }
    }
}
#[test]
fn shared_frames_wait_for_exact_credits_and_resize_cannot_unfreeze_a_tab() {
    let store = wega_platform::FrameStore::create(protocol::MAX_PIXELS * 4).unwrap();
    let mut worker = Worker::start(store.path().unwrap());
    let first = worker.frame();
    worker.send(Command::Resize {
        width: 64,
        height: 64,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    });
    let second = worker.frame();
    assert_ne!(first.0, second.0);
    worker.send(Command::Redraw);
    worker.no_frame();
    worker.send(Command::FrameConsumed {
        slot: first.0,
        sequence: first.1.wrapping_add(1),
    });
    worker.no_frame();
    worker.send(Command::FrameConsumed {
        slot: first.0,
        sequence: first.1,
    });
    let third = worker.frame();
    assert_eq!(third.0, first.0);
    assert_ne!(third.1, first.1);
    worker.send(Command::Freeze(true));
    worker.send(Command::FrameConsumed {
        slot: third.0,
        sequence: third.1,
    });
    worker.send(Command::FrameConsumed {
        slot: second.0,
        sequence: second.1,
    });
    worker.send(Command::Resize {
        width: 80,
        height: 64,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
    });
    worker.send(Command::Redraw);
    worker.no_frame();
    worker.send(Command::Freeze(false));
    worker.frame();
    worker.send(Command::Load {
        url: "about:blank".into(),
        serial: 9,
    });
    worker
        .until(|event| matches!(event,Event::State{serial:9,rss_bytes:Some(bytes),..} if *bytes>0));
}
