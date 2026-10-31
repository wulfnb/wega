use super::*;
use std::net::TcpListener;

struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    body: Arc<Vec<u8>>,
    ranges: Arc<AtomicUsize>,
    concurrent: Arc<AtomicUsize>,
}
impl Server {
    fn new(ignore_range: bool, slow: bool) -> Self {
        Self::with_range_fault(ignore_range, slow, 0)
    }
    fn with_range_fault(ignore_range: bool, slow: bool, fault: u8) -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let url = format!("http://{}/archive.bin", socket.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let body = Arc::new(
            (0..(6 * 1024 * 1024 + 173))
                .map(|i| (i % 251) as u8)
                .collect::<Vec<_>>(),
        );
        let data = body.clone();
        let ranges = Arc::new(AtomicUsize::new(0));
        let count = ranges.clone();
        let concurrent = Arc::new(AtomicUsize::new(0));
        let maximum = concurrent.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let worker = thread::spawn(move || {
            let mut workers = Vec::new();
            while !stopping.load(Ordering::Relaxed) {
                match socket.accept() {
                    Ok((mut stream, _)) => {
                        let data = data.clone();
                        let count = count.clone();
                        let maximum = maximum.clone();
                        let active = active.clone();
                        workers.push(thread::spawn(move||{
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();let mut request=Vec::new();let mut byte=[0];while request.len()<16*1024&&stream.read(&mut byte).unwrap_or(0)==1{request.push(byte[0]);if request.ends_with(b"\r\n\r\n"){break}}
            let text=String::from_utf8_lossy(&request);let head=text.starts_with("HEAD ");
            let range=text.lines().find_map(|s|s.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_owned)).and_then(|r|r.split_once('-').and_then(|(a,b)|Some((a.parse::<usize>().ok()?,b.parse::<usize>().ok()?))));
            let (start,end,status)=if !ignore_range&&let Some((a,b))=range{count.fetch_add(1,Ordering::Relaxed);(a,b,"206 Partial Content")}else{(0,data.len()-1,"200 OK")};
            let extra=if status.starts_with("206"){format!("Content-Range: bytes {}-{end}/{}\r\n",if fault == 1 {start + 1} else {start},data.len())}else{String::new()};
            let etag = if status.starts_with("206") && fault == 2 { "changed-v2" } else { "stable-v1" };
            let header=format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: \"{etag}\"\r\nContent-Disposition: attachment; filename=archive.bin\r\n{extra}Connection: close\r\n\r\n",end-start+1);
            if stream.write_all(header.as_bytes()).is_err()||head{return}
            let n=active.fetch_add(1,Ordering::Relaxed)+1;maximum.fetch_max(n,Ordering::Relaxed);
            thread::sleep(Duration::from_millis(80));
            for chunk in data[start..=end].chunks(32768){if stream.write_all(chunk).is_err(){break}
                        if slow{thread::sleep(Duration::from_millis(5));}}
            active.fetch_sub(1,Ordering::Relaxed);
        }));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(5)),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            url,
            stop,
            worker: Some(worker),
            body,
            ranges,
            concurrent,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn manager() -> (Manager, PathBuf) {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "wega-download-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut m = Manager::new(&root);
    m.directory = root.join("saved");
    (m, root)
}
fn finished(job: &Job) -> Progress {
    let start = Instant::now();
    loop {
        let p = job.progress.lock().unwrap().clone();
        if matches!(p.phase, Phase::Complete | Phase::Failed | Phase::Cancelled) {
            return p;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "download timed out: {:?}",
            p.phase
        );
        thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn multipart_is_parallel_and_bytes_match() {
    let server = Server::new(false, false);
    let (mut m, root) = manager();
    m.start(&server.url, "", 4, String::new()).unwrap();
    let job = m.jobs().pop().unwrap();
    let p = finished(&job);
    assert_eq!(p.phase, Phase::Complete, "{}", p.message);
    assert_eq!(fs::read(p.path.unwrap()).unwrap(), *server.body);
    assert!(server.ranges.load(Ordering::Relaxed) >= 3);
    assert!(server.concurrent.load(Ordering::Relaxed) >= 2);
    drop(m);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn ignored_range_falls_back_without_corruption() {
    let server = Server::new(true, false);
    let (mut m, root) = manager();
    m.start(&server.url, "../unsafe.bin", 4, String::new())
        .unwrap();
    let p = finished(&m.jobs()[0]);
    assert_eq!(p.phase, Phase::Complete, "{}", p.message);
    let path = p.path.unwrap();
    assert_eq!(path.parent(), Some(m.directory.as_path()));
    assert_eq!(fs::read(path).unwrap(), *server.body);
    assert_eq!(p.connections, 1);
    drop(m);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn pause_resume_and_cancel_do_not_publish_partial_files() {
    let server = Server::new(false, true);
    let (mut m, root) = manager();
    m.start(&server.url, "paused.bin", 2, String::new())
        .unwrap();
    let job = m.jobs()[0].clone();
    thread::sleep(Duration::from_millis(250));
    job.pause();
    let start = Instant::now();
    while job.progress.lock().unwrap().phase != Phase::Paused {
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read_dir(&m.directory).unwrap().count(), 0);
    m.resume(&job).unwrap();
    assert_eq!(finished(&job).phase, Phase::Complete);
    m.start(&server.url, "cancelled.bin", 2, String::new())
        .unwrap();
    let job = m.jobs()[1].clone();
    thread::sleep(Duration::from_millis(150));
    job.cancel();
    assert_eq!(finished(&job).phase, Phase::Cancelled);
    assert!(!m.directory.join("cancelled.bin").exists());
    drop(m);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn filenames_and_urls_are_safe() {
    assert!(validate_url("file:///etc/passwd").is_err());
    assert!(validate_url("https://user:secret@example.com/a").is_err());
    assert_eq!(safe_filename("../../a\\b\n.txt"), "_.._a_b_.txt");
    assert_eq!(safe_filename("con.txt"), "_con.txt");
    assert_eq!(safe_filename("NUL"), "_NUL");
    assert_eq!(safe_filename("prn.pdf"), "_prn.pdf");
    assert_eq!(decode("%aé"), "%aé");
    assert_eq!(decode("caf%C3%A9+one"), "café+one");
    let root = std::env::temp_dir().join(format!("wega-publish-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let body = root.join("source");
    fs::write(&body, b"payload").unwrap();
    assert_ne!(
        publish(&body, &root, "a.bin").unwrap(),
        publish(&body, &root, "a.bin").unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn attachment_names_allow_legacy_headers_and_utf8_encoding() {
    let (_, root) = manager();
    fs::create_dir_all(&root).unwrap();
    let path = root.join("headers");
    fs::write(&path,b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 42\r\nContent-Disposition: attachment; filename=\"caf\xe9.bin\"\r\n\r\n").unwrap();
    let meta = headers(&path).unwrap();
    assert_eq!(meta.length(), Some(42));
    assert_eq!(meta.filename().as_deref(), Some("café.bin"));
    fs::write(&path,b"HTTP/1.1 200 OK\r\nContent-Disposition: attachment; filename*=UTF-8''caf%C3%A9.bin\r\n\r\n").unwrap();
    assert_eq!(
        headers(&path).unwrap().filename().as_deref(),
        Some("café.bin")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn admission_limit_and_resume_failure_preserve_worker_state() {
    let server = Server::new(false, true);
    let (mut manager, root) = manager();
    for _ in 0..3 {
        manager.start(&server.url, "", 1, String::new()).unwrap();
    }
    assert!(manager.start(&server.url, "", 1, String::new()).is_err());
    let paused = manager.jobs()[0].clone();
    paused.pause();
    manager.start(&server.url, "", 1, String::new()).unwrap();
    assert!(manager.resume(&paused).is_err());
    assert_eq!(paused.command.load(Ordering::Relaxed), PAUSE);
    for job in manager.jobs() {
        job.cancel();
    }
    drop(manager);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_metadata_never_enables_unsafe_resume_or_names() {
    let mut meta = Meta::default();
    assert_eq!(meta.length(), None);
    assert_eq!(meta.validator(), None);
    assert_eq!(meta.filename(), None);
    meta.headers.insert("etag".into(), "W/\"weak\"".into());
    meta.headers.insert(
        "last-modified".into(),
        "Wed, 21 Oct 2026 12:00:00 GMT".into(),
    );
    assert_eq!(
        meta.validator().as_deref(),
        Some("Wed, 21 Oct 2026 12:00:00 GMT")
    );
    meta.headers
        .insert("content-length".into(), "not-a-length".into());
    assert_eq!(meta.length(), None);
    meta.headers.insert(
        "content-disposition".into(),
        "attachment; filename*=utf-8''%2Ftmp%2Fevil.txt".into(),
    );
    assert_eq!(meta.filename().as_deref(), Some("_tmp_evil.txt"));
    assert_eq!(safe_filename(" . "), "download.bin");
    assert_eq!(safe_filename(&"é".repeat(200)).len(), 180);
    assert!(validate_url(&format!("https://example.com/{}", "x".repeat(8192))).is_err());
    assert!(validate_url("not a URL").is_err());
    assert_eq!(
        url_filename(&url::Url::parse("https://example.com/").unwrap()),
        "download.bin"
    );
}

#[test]
fn malformed_header_files_and_publication_errors_are_reported() {
    let (_, root) = manager();
    fs::create_dir_all(&root).unwrap();
    assert!(headers(&root.join("missing")).is_err());
    assert!(headers(&root).is_err());
    let header = root.join("headers");
    fs::write(&header, b"HTTP/1.1 302 Found\r\nContent-Length: 23\r\n\r\nHTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\n").unwrap();
    let meta = headers(&header).unwrap();
    assert_eq!(meta.status, 404);
    assert_eq!(
        meta.length(),
        None,
        "Redirect length must not leak into the final response"
    );
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    assert!(publish(&root.join("missing"), &target, "file.bin").is_err());
    assert_eq!(
        fs::read_dir(&target).unwrap().count(),
        0,
        "Failed publication cleans temporary files"
    );
    assert!(publish(&header, &root.join("missing-target"), "file.bin").is_err());
    assert!(clear_parts(&root.join("missing-work")).is_err());
    fs::create_dir(root.join("part-is-a-directory")).unwrap();
    assert!(clear_parts(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn transfer_failure_is_visible_and_finished_jobs_can_be_cleared() {
    // A zero-byte server response makes real curl fail without fabricating a
    // process exit status. The listener is private and short lived.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/broken.bin", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut byte = [0];
            let mut request = Vec::new();
            while stream.read(&mut byte).unwrap_or(0) == 1 {
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(
                    b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        }
    });
    let (mut manager, root) = manager();
    manager.start(&url, "", 1, String::new()).unwrap();
    let job = manager.jobs()[0].clone();
    let result = finished(&job);
    assert_eq!(result.phase, Phase::Failed);
    assert!(!result.message.is_empty());
    assert!(result.path.is_none());
    assert_eq!(job.bytes(), 0);
    assert_eq!(job.speed(), 0.0);
    manager.clear_finished();
    assert!(manager.jobs().is_empty());
    worker.join().unwrap();
    drop(manager);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn saturated_destination_names_do_not_overwrite_any_file() {
    let (_, root) = manager();
    fs::create_dir_all(&root).unwrap();
    let body = root.join("source");
    fs::write(&body, b"new").unwrap();
    for index in 0..10000 {
        let name = if index == 0 {
            "file.bin".into()
        } else {
            format!("file ({index}).bin")
        };
        fs::write(root.join(name), b"existing").unwrap();
    }
    assert!(
        publish(&body, &root, "file.bin")
            .unwrap_err()
            .contains("Too many files")
    );
    assert_eq!(fs::read(root.join("file.bin")).unwrap(), b"existing");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 10001);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn application_download_port_uses_the_real_service_validation() {
    let (mut manager, _) = manager();
    assert!(
        DownloadPort::start(&mut manager, "file:///private", "file", 1, String::new()).is_err()
    );
    assert!(manager.jobs().is_empty());
}

#[test]
fn invalid_ranges_and_changed_validators_fall_back_to_verified_single_transfer() {
    for fault in [1, 2] {
        let server = Server::with_range_fault(false, false, fault);
        let (mut manager, root) = manager();
        manager
            .start(&server.url, "fault.bin", 3, String::new())
            .unwrap();
        let progress = finished(&manager.jobs()[0]);
        assert_eq!(progress.phase, Phase::Complete, "{}", progress.message);
        assert_eq!(
            progress.connections, 1,
            "Invalid range metadata must disable multipart"
        );
        assert_eq!(fs::read(progress.path.unwrap()).unwrap(), *server.body);
        assert!(server.ranges.load(Ordering::Relaxed) > 0);
        drop(manager);
        fs::remove_dir_all(root).unwrap();
    }
}

fn stored_job(phase: Phase) -> Arc<Job> {
    Arc::new(Job {
        url: "http://127.0.0.1/retained.bin".into(),
        progress: Mutex::new(Progress {
            phase,
            filename: "retained.bin".into(),
            total: None,
            connections: 1,
            message: String::new(),
            path: None,
        }),
        command: AtomicU8::new(RUN),
        completed: AtomicU64::new(0),
        inflight: (0..8).map(|_| AtomicU64::new(0)).collect(),
        started: Instant::now(),
    })
}

#[test]
fn download_history_capacity_preserves_in_progress_and_paused_jobs() {
    let (mut manager, _) = manager();
    for phase in [
        Phase::Connecting,
        Phase::Downloading,
        Phase::Paused,
        Phase::Complete,
        Phase::Failed,
        Phase::Cancelled,
    ] {
        manager.items.push(Item {
            job: stored_job(phase),
            worker: None,
        });
    }
    // Removing one entry only forgets finished ones.
    let jobs = manager.jobs();
    manager.remove(&jobs[1]);
    manager.remove(&jobs[3]);
    assert_eq!(manager.jobs().len(), 5);
    assert!(!manager.jobs().iter().any(|j| Arc::ptr_eq(j, &jobs[3])));
    manager.clear_finished();
    assert_eq!(
        manager
            .jobs()
            .iter()
            .map(|j| j.progress.lock().unwrap().phase.clone())
            .collect::<Vec<_>>(),
        vec![Phase::Connecting, Phase::Downloading, Phase::Paused]
    );
    manager.items.clear();
    for _ in 0..64 {
        manager.items.push(Item {
            job: stored_job(Phase::Complete),
            worker: None,
        });
    }
    assert!(
        manager
            .start("http://127.0.0.1/file.bin", "", 1, String::new())
            .unwrap_err()
            .contains("Clear finished")
    );
    manager.clear_finished();
    assert!(manager.jobs().is_empty());
}

#[test]
fn multipart_reuses_complete_segments_and_bounds_work_before_launching_workers() {
    let (_, root) = manager();
    fs::create_dir_all(&root).unwrap();
    let job = stored_job(Phase::Paused);
    let mut meta = Meta::default();
    meta.headers
        .insert("content-length".into(), (SEGMENT * 100_001).to_string());
    assert!(
        parallel(&job, &root, &meta, 8)
            .unwrap_err()
            .contains("Too many")
    );
    meta.headers.insert("content-length".into(), "4".into());
    meta.headers.insert("etag".into(), "\"stable\"".into());
    fs::write(root.join("part-000000"), b"data").unwrap();
    let path = parallel(&job, &root, &meta, 2).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"data");
    assert_eq!(job.bytes(), 4);
    job.cancel();
    assert!(
        parallel(&job, &root, &meta, 2)
            .unwrap_err()
            .contains("Interrupted")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn published_files_are_marked_and_leave_no_temporary_files() {
    let server = Server::new(false, false);
    let (mut m, root) = manager();
    let marks = Arc::new(Mutex::new(Vec::new()));
    let seen = marks.clone();
    m.marker = Some(Arc::new(move |path: &Path, url: &str| {
        seen.lock()
            .unwrap()
            .push((path.to_path_buf(), url.to_owned()));
    }));
    m.start(&server.url, "", 1, String::new()).unwrap();
    let p = finished(&m.jobs().pop().unwrap());
    assert_eq!(p.phase, Phase::Complete, "{}", p.message);
    assert_eq!(
        *marks.lock().unwrap(),
        [(p.path.clone().unwrap(), server.url.clone())]
    );
    let leftovers: Vec<_> = fs::read_dir(&m.directory)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    drop(m);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_poisoned_progress_lock_does_not_panic_readers() {
    let job = Arc::new(Job {
        url: "https://example.com/".into(),
        progress: Mutex::new(Progress {
            phase: Phase::Downloading,
            filename: "a".into(),
            total: None,
            connections: 1,
            message: String::new(),
            path: None,
        }),
        command: AtomicU8::new(RUN),
        completed: AtomicU64::new(0),
        inflight: Vec::new(),
        started: Instant::now(),
    });
    let poisoner = job.clone();
    let _ = thread::spawn(move || {
        let _guard = poisoner.progress.lock().unwrap();
        panic!("worker panic while holding progress");
    })
    .join();
    assert!(job.progress.is_poisoned());
    assert_eq!(job.snapshot().phase, Phase::Downloading);
    job.phase(Phase::Failed, "recovered");
    assert_eq!(job.snapshot().message, "recovered");
}

#[test]
fn curl_is_resolved_from_system_directories_only() {
    for candidate in curl_candidates() {
        assert!(candidate.is_absolute(), "{}", candidate.display());
    }
    if let Some(path) = curl_program() {
        assert!(path.is_absolute() && path.is_file());
    }
}

#[test]
fn mark_of_the_web_metadata_and_program_detection() {
    assert_eq!(
        zone_identifier("https://example.com/a.exe\r\nZoneId=0"),
        "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://example.com/a.exeZoneId=0\r\nHostUrl=https://example.com/a.exeZoneId=0\r\n"
    );
    assert_eq!(quarantine_value(0x6500_0000, "Wega"), "0081;65000000;Wega;");
    for name in [
        "Setup.EXE",
        "a.app",
        "run.sh",
        "x.command",
        "i.pkg",
        "d.dmg",
        "s.ps1",
        "j.jar",
        "l.desktop",
        "b.AppImage",
    ] {
        assert!(opens_as_program(Path::new(name)), "{name}");
    }
    for name in ["photo.png", "doc.pdf", "archive.zip", "README", "notes.txt"] {
        assert!(!opens_as_program(Path::new(name)), "{name}");
    }
}
