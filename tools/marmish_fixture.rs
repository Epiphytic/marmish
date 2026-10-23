//! Bounded native CI fixture. No interpreter, arbitrary paths or program execution.
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

fn number(value: Option<OsString>, maximum: u64) -> io::Result<u64> {
    value
        .and_then(|x| x.into_string().ok())
        .and_then(|x| x.parse().ok())
        .filter(|&n| n <= maximum)
        .ok_or_else(|| io::Error::other("invalid_number"))
}

fn run() -> io::Result<i32> {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref().and_then(|x| x.to_str()) {
        Some("argv") => {
            let mut out = io::stdout().lock();
            for arg in args {
                let bytes = arg.as_bytes();
                if bytes.len() > 16384 {
                    return Err(io::Error::other("oversized"));
                }
                out.write_all(&(bytes.len() as u32).to_be_bytes())?;
                out.write_all(bytes)?;
            }
            io::stderr().write_all(b"stderr\0\xff")?;
            Ok(7)
        }
        Some("binary") => {
            let mut data = Vec::new();
            io::stdin().take(16385).read_to_end(&mut data)?;
            if data.len() > 16384 {
                return Err(io::Error::other("oversized"));
            }
            io::stdout().write_all(&data)?;
            data.reverse();
            io::stderr().write_all(&data)?;
            Ok(0)
        }
        Some("count") => {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open("executed")?;
            file.write_all(b"once\n")?;
            file.sync_all()?;
            io::stdout().write_all(b"once\n")?;
            Ok(0)
        }
        Some("sleep") => {
            let seconds = number(args.next(), 30)?;
            if args.next().is_some() {
                return Err(io::Error::other("invalid_arguments"));
            }
            std::thread::sleep(Duration::from_secs(seconds));
            io::stdout().write_all(b"survived\n")?;
            Ok(0)
        }
        Some("marker") => {
            let value = args
                .next()
                .ok_or_else(|| io::Error::other("missing_marker"))?;
            let bytes = value.as_bytes();
            if args.next().is_some()
                || bytes.len() > 512
                || bytes.is_empty()
                || !bytes
                    .iter()
                    .all(|x| x.is_ascii_alphanumeric() || b":-".contains(x))
            {
                return Err(io::Error::other("invalid_marker"));
            }
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open("peer-completed")?;
            file.write_all(bytes)?;
            file.sync_all()?;
            io::stdout().write_all(b"marked\n")?;
            Ok(0)
        }
        Some("noop") if args.next().is_none() => Ok(0),
        _ => Err(io::Error::other("invalid_arguments")),
    }
}

fn main() {
    unsafe extern "C" {
        fn alarm(seconds: u32) -> u32;
    }
    // Even a broken/disconnected stdin stream cannot strand a test command.
    unsafe {
        alarm(45);
    }
    match run() {
        Ok(code) => std::process::exit(code),
        Err(_) => {
            eprintln!("fixture_failed");
            std::process::exit(125);
        }
    }
}
