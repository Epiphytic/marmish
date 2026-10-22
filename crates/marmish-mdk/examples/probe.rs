use marmish_mdk::RealWn;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let executable = args.next().ok_or("usage: probe /absolute/path/to/wn")?;
    if args.next().is_some() {
        return Err("only an explicit executable path is accepted".into());
    }
    let observation = RealWn::new(PathBuf::from(executable))?.version()?;
    println!("Unverified binary observation; no runtime provenance or live egress approval.");
    print!("{}", observation.stdout);
    eprint!("{}", observation.stderr);
    Ok(())
}
