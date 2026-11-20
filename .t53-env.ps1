# Task 53 build environment: pinned 1.99.1 toolchain, LLVM clang-cl and the MSVC
# header/library environment (rocksdb builds through the `cc` crate).
Set-Location 'd:\Git\neo-rs'

$b = "$env:USERPROFILE\.rustup\toolchains\1.99.1-x86_64-pc-windows-msvc\bin"
$env:PATH = "$b;C:\Program Files\LLVM\bin;$env:PATH"
$env:RUSTC = "$b\rustc.exe"
$env:RUSTDOC = "$b\rustdoc.exe"
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:CC = 'C:\Program Files\LLVM\bin\clang-cl.exe'
$env:CXX = 'C:\Program Files\LLVM\bin\clang-cl.exe'

if (-not $env:INCLUDE) {
    cmd /c '"D:\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && set' |
        ForEach-Object {
            if ($_ -match '^([^=]+)=(.*)$') {
                Set-Item -Path "env:$($matches[1])" -Value $matches[2]
            }
        }
}
