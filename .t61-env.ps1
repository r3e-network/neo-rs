$tc = "$env:USERPROFILE\.rustup\toolchains\1.99.1-x86_64-pc-windows-msvc\bin"
$env:PATH = "$tc;C:\Program Files\LLVM\bin;$env:PATH"
$env:RUSTC = "$tc\rustc.exe"
$env:RUSTDOC = "$tc\rustdoc.exe"
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:CC = 'clang-cl'
$env:CXX = 'clang-cl'
Set-Location d:\Git\neo-rs
