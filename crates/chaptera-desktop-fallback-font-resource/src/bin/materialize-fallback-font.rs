use chaptera_desktop_fallback_font_resource as resource;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

fn fail(message: impl AsRef<str>) -> ! {
    eprintln!("{}", message.as_ref());
    process::exit(2);
}

fn main() {
    let mut args = env::args_os();
    let _program = args.next();
    let output = match args.next() {
        Some(path) => PathBuf::from(path),
        None => fail("usage: materialize-fallback-font <output-path>"),
    };
    if args.next().is_some() {
        fail("usage: materialize-fallback-font <output-path>");
    }

    if let Err(error) = resource::validate() {
        fail(error);
    }
    if let Some(parent) = output.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            fail(format!("cannot create fallback-font parent directory: {error}"));
        }
    }
    if let Err(error) = fs::write(&output, resource::bytes()) {
        fail(format!("cannot write pinned fallback font: {error}"));
    }
}
