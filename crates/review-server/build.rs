use std::{env, fs, path::Path};
fn main() {
    println!("cargo:rerun-if-changed=../../dist");
    let output = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("ui");
    fs::create_dir_all(&output).unwrap();
    let source = Path::new("../../dist");
    if source.join("index.html").exists() {
        copy(source, &output);
    } else {
        if env::var("PROFILE").as_deref() == Ok("release") {
            panic!("Build the UI before installing: run just install");
        }
        fs::write(output.join("index.html"),"<!doctype html><title>DevDeck</title><p>Build the browser UI with <code>just build-ui</code>, then rebuild the server.</p>").unwrap();
    }
}
fn copy(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&dest).unwrap();
            copy(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}
