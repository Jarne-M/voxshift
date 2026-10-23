@echo off
rem Builds target\release\voxshift.exe with the portable toolchain in ..\.toolchain.
rem Double-click it, or run "build" in a terminal in this folder. Quit VoxShift first:
rem Windows will not let the build replace a running exe.
setlocal
cd /d "%~dp0"
set "T=%~dp0..\.toolchain"
set "RUSTUP_HOME=%T%\rustup"
set "CARGO_HOME=%T%\cargo"
set "PATH=%T%\cargo\bin;%T%\mingw\mingw64\bin;%T%\git\cmd;%PATH%"
cargo build --release
if errorlevel 1 (
    echo.
    echo Build failed. If it says "Access is denied", quit VoxShift ^(tray icon, Quit^) and try again.
) else (
    echo.
    echo Done: %~dp0target\release\voxshift.exe
)
pause
