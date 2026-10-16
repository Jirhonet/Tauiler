#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
#[cfg(windows)]
mod creation;
mod diagnostics;
mod input;
mod layout;
#[cfg(windows)]
mod native;
#[cfg(windows)]
mod tray_icon;

fn main() {
    #[cfg(all(windows, not(debug_assertions)))]
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
    if let Err(error) = run() {
        report!("Tauiler: {error}");
        #[cfg(all(windows, not(debug_assertions)))]
        if !std::env::args().any(|a| matches!(a.as_str(), "--check" | "--inspect" | "--self-test"))
        {
            let text: Vec<u16> =
                format!("{error}\n\nSee %APPDATA%\\Tauiler\\tauiler.log for diagnostics.")
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    text.as_ptr(),
                    windows_sys::core::w!("Tauiler"),
                    windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                        | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                );
            }
        }
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Tauiler — Windows tiling manager\n\nUsage: tauiler [--config PATH] [--check | --inspect | --self-test]\n\n--check validates configuration without managing windows.\n--inspect lists eligible windows and monitors without changing them.\n--self-test runs an isolated native-window integration test.\n\nSUPER+V toggles floating. SUPER+M or clicking the numbered tray icon pauses/resumes automatic tiling. Bindings remain active. Right-click the tray icon to exit."
        );
        return Ok(());
    }
    let path = match args.iter().position(|a| a == "--config") {
        Some(i) => std::path::PathBuf::from(args.get(i + 1).ok_or("--config needs a path")?),
        None => config::default_path(),
    };
    if args.iter().any(|a| {
        !matches!(
            a.as_str(),
            "--config" | "--check" | "--inspect" | "--self-test"
        ) && a != &path.to_string_lossy()
    }) {
        return Err("unknown argument; use --help".into());
    }
    let settings = config::load(&path)?;
    if args.iter().any(|a| a == "--check") {
        println!(
            "Configuration valid: {} ({} bindings)",
            path.display(),
            settings.bindings.len()
        );
        return Ok(());
    }
    #[cfg(windows)]
    return native::run(settings, path, &args);
    #[cfg(not(windows))]
    Err("Tauiler requires Windows 11; --check is available on other platforms".into())
}
