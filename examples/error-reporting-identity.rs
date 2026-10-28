//! Initialize a profile identity and print only its public enrollment document.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let profile = arguments
        .next()
        .map(std::path::PathBuf::from)
        .ok_or("Usage: error-reporting-identity PROFILE_DIRECTORY [--queue-probe]")?;
    wega::error_reporting::Identity::load(&profile)?;
    if let Some(option) = arguments.next() {
        if option != "--queue-probe" || arguments.next().is_some() {
            return Err("Expected optional --queue-probe".into());
        }
        let mut diagnostic = wega::diagnostic_event::Diagnostic::error(
            wega::diagnostic_event::Category::Browser,
            "Error reporting setup verification (synthetic test).",
            None,
        );
        diagnostic.severity = "warning".into();
        wega::error_reporting::Reporter::buffer(&profile)?
            .enqueue("browser:setup-probe", diagnostic)?;
    }
    println!(
        "{}",
        std::fs::read_to_string(profile.join("error-reporting/enrollment.json"))?
    );
    Ok(())
}
