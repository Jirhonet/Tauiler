use std::{
    fmt::Arguments,
    fs::{File, OpenOptions},
    io::Write,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
static LOG: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn write(args: Arguments<'_>) {
    eprintln!("{args}");
    let log = LOG.get_or_init(|| {
        let path = crate::config::default_path().with_file_name("tauiler.log");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000) {
            let _ = std::fs::rename(&path, path.with_extension("previous.log"));
        }
        Mutex::new(OpenOptions::new().create(true).append(true).open(path).ok())
    });
    if let Ok(mut file) = log.lock()
        && let Some(file) = file.as_mut()
    {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(file, "[{timestamp}] {args}");
    }
}

#[macro_export]
macro_rules! report { ($($arg:tt)*) => { $crate::diagnostics::write(format_args!($($arg)*)) }; }
