use std::process::Command;

fn main() {
    // Run a simple cargo check to see what's working
    let output = Command::new("cargo")
        .args(&["check", "--package", "neo-network", "--lib"])
        .output()
        .expect("Failed to execute cargo check");

    println!("Status: {}", output.status);
    println!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
    println!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
}