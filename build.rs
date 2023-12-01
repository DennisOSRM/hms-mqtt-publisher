use std::process::Command;
fn main() {
    // runs git describe --always --dirty
    if let Ok(output) = Command::new("git")
        .args(["describe", "--always", "--dirty"])
        .output()
    {
        let git_hash = String::from_utf8(output.stdout).unwrap();
        println!("cargo:rustc-env=GIT_HASH={git_hash}");
    } else {
        println!("cargo:rustc-env=GIT_HASH=UNKNOWN");
    }
}
