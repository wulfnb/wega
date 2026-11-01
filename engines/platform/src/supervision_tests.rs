use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
};

#[test]
fn child_helper() {
    if std::env::var_os("WEGA_SUPERVISION_HELPER").is_none() {
        return;
    }
    let bytes = std::env::var("WEGA_SUPERVISION_ALLOCATE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
        .min(64 * 1024 * 1024);
    let allocation = vec![1_u8; bytes];
    if bytes != 0 {
        println!("WEGA_ALLOCATED");
        std::io::stdout().flush().unwrap();
    }
    let mut text = String::new();
    let _ = std::io::stdin().read_line(&mut text);
    std::hint::black_box(allocation);
}

fn allocated() -> OwnedProcess {
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "supervision::tests::child_helper", "--nocapture"])
        .env("WEGA_SUPERVISION_HELPER", "1")
        .env("WEGA_SUPERVISION_ALLOCATE", (64 * 1024 * 1024).to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut process = OwnedProcess::new(child, |_| {});
    let output = process.take_stdout().unwrap();
    let (ready, received) = mpsc::channel();
    std::thread::spawn(move || {
        let mut ready = Some(ready);
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if line.contains("WEGA_ALLOCATED")
                && let Some(ready) = ready.take()
            {
                let _ = ready.send(());
            }
        }
    });
    received.recv_timeout(Duration::from_secs(5)).unwrap();
    process
}

#[test]
fn aggregate_pressure_stops_one_consumer_without_treating_a_pending_kill_as_free_memory() {
    let mut first = allocated();
    let mut second = allocated();
    let _first_input = first.take_stdin().unwrap();
    let _second_input = second.take_stdin().unwrap();
    let sample = |process: &OwnedProcess| match process.control().sample().unwrap() {
        Sample::Alive {
            resident_bytes: Some(bytes),
        } => bytes,
        sample => panic!("Expected native residency: {sample:?}"),
    };
    let first_bytes = sample(&first);
    let second_bytes = sample(&second);
    let limit = first_bytes.max(second_bytes) + 32 * 1024 * 1024;
    assert!(first_bytes + second_bytes > limit);
    let owner = Supervisor::new(Budgets {
        worker_resident_bytes: limit,
        aggregate_resident_bytes: limit,
        ..Budgets::default()
    })
    .unwrap();
    let first_ticket = owner.reserve(0).unwrap();
    let second_ticket = owner.reserve(0).unwrap();
    let (fault, faults) = mpsc::channel();
    let other_fault = fault.clone();
    first_ticket
        .attach(&mut first, move |reason| {
            let _ = fault.send(reason);
        })
        .unwrap();
    second_ticket
        .attach(&mut second, move |reason| {
            let _ = other_fault.send(reason);
        })
        .unwrap();
    first_ticket.ready();
    second_ticket.ready();
    assert_eq!(
        faults.recv_timeout(Duration::from_secs(5)).unwrap(),
        Violation::AggregateMemory
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let exited = [
            first.control().sample().unwrap(),
            second.control().sample().unwrap(),
        ]
        .into_iter()
        .filter(|sample| *sample == Sample::Exited)
        .count();
        if exited != 0 {
            assert_eq!(exited, 1);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    owner.poll(Instant::now());
    assert!(faults.try_recv().is_err());
}
fn child() -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "supervision::tests::child_helper", "--nocapture"])
        .env("WEGA_SUPERVISION_HELPER", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}
#[test]
fn frame_reservations_are_bounded_and_return_on_abandoned_startup() {
    let owner = Supervisor::new(Budgets {
        workers: 2,
        worker_frame_bytes: 8,
        aggregate_frame_bytes: 8,
        ..Budgets::default()
    })
    .unwrap();
    assert!(owner.reserve(9).is_err());
    let first = owner.reserve(8).unwrap();
    assert!(owner.reserve(1).is_err());
    let id = first.0.id;
    drop(first);
    let second = owner.reserve(8).unwrap();
    assert!(second.0.id > id);
    owner.state.lock().unwrap().next = u64::MAX;
    assert!(owner.reserve(0).is_err());
}
#[test]
fn a_process_reservation_survives_page_drop_until_exit_and_reaping() {
    let owner = Supervisor::new(Budgets {
        workers: 1,
        ..Budgets::default()
    })
    .unwrap();
    let ticket = owner.reserve(8).unwrap();
    let (exit, exited) = mpsc::channel();
    let mut process = OwnedProcess::new(child(), move |report| {
        exit.send(report).unwrap();
    });
    let mut input = process.take_stdin().unwrap();
    ticket.attach(&mut process, |_| {}).unwrap();
    ticket.ready();
    drop(ticket); // The real process reaper, not the Page, still owns the reservation.
    assert!(owner.reserve(0).is_err());
    input.write_all(b"exit\n").unwrap();
    drop(input);
    process.allow_graceful_shutdown(crate::process::MAX_SHUTDOWN_GRACE);
    drop(process);
    assert!(
        exited
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .status
            .unwrap()
            .success()
    );
    crate::process::wait_for_shutdowns();
    assert!(owner.state.lock().unwrap().entries.is_empty());
    assert!(owner.reserve(8).is_ok());
}
#[test]
fn host_memory_sampling_terminates_a_worker_and_keeps_presentation_reservations() {
    let owner = Supervisor::new(Budgets {
        worker_resident_bytes: 1,
        aggregate_resident_bytes: 1,
        ..Budgets::default()
    })
    .unwrap();
    let ticket = owner.reserve(8).unwrap();
    let (fault, faults) = mpsc::channel();
    let (exit, exits) = mpsc::channel();
    let mut process = OwnedProcess::new(child(), move |report| {
        exit.send(report).unwrap();
    });
    let _input = process.take_stdin().unwrap();
    ticket
        .attach(&mut process, move |reason| {
            fault.send(reason).unwrap();
        })
        .unwrap();
    ticket.ready();
    assert_eq!(
        faults.recv_timeout(Duration::from_secs(3)).unwrap(),
        Violation::ResidentMemory
    );
    let control = process.control();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(control.sample().unwrap(), Sample::Exited) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(process);
    assert!(exits.recv_timeout(Duration::from_secs(3)).unwrap().forced);
    crate::process::wait_for_shutdowns();
    assert_eq!(
        owner
            .state
            .lock()
            .unwrap()
            .entries
            .get(&ticket.0.id)
            .unwrap()
            .frames,
        8
    );
    drop(ticket);
    assert!(owner.state.lock().unwrap().entries.is_empty());
}
#[test]
fn startup_timeout_cannot_be_cancelled_by_a_late_ready_and_only_notifies_once() {
    let owner = Supervisor::new(Budgets {
        startup_timeout: Duration::from_millis(20),
        ..Budgets::default()
    })
    .unwrap();
    let ticket = owner.reserve(0).unwrap();
    let (fault, faults) = mpsc::channel();
    let mut process = OwnedProcess::new(child(), |_| {});
    let _input = process.take_stdin().unwrap();
    ticket
        .attach(&mut process, move |reason| {
            fault.send(reason).unwrap();
        })
        .unwrap();
    owner.poll(Instant::now() + Duration::from_secs(1));
    ticket.ready();
    assert_eq!(
        faults.recv_timeout(Duration::from_secs(3)).unwrap(),
        Violation::StartupTimeout
    );
    assert!(
        !owner
            .state
            .lock()
            .unwrap()
            .entries
            .get(&ticket.0.id)
            .unwrap()
            .ready
    );
    owner.poll(Instant::now() + Duration::from_secs(2));
    assert!(faults.try_recv().is_err());
}
