# Dot-source in a PowerShell session: . .\env.ps1
# Uses the portable toolchain in ..\.toolchain (Rust GNU target, MinGW, MinGit).
$t = Resolve-Path "$PSScriptRoot\..\.toolchain"
$env:RUSTUP_HOME = "$t\rustup"
$env:CARGO_HOME = "$t\cargo"
$env:PATH = "$t\cargo\bin;$t\mingw\mingw64\bin;$t\git\cmd;$env:PATH"
