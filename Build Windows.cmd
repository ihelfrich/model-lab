@echo off
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Install Rust stable and the Visual Studio C++ build tools, then rerun this file.
  pause
  exit /b 1
)
cargo test --locked
if errorlevel 1 exit /b 1
cargo build --release --locked
if errorlevel 1 exit /b 1
if not exist dist mkdir dist
if defined CARGO_TARGET_DIR (
  copy /y "%CARGO_TARGET_DIR%\release\tree-ensemble-lab.exe" "dist\Tree Ensemble Lab.exe"
) else (
  copy /y "target\release\tree-ensemble-lab.exe" "dist\Tree Ensemble Lab.exe"
)
if errorlevel 1 exit /b 1
echo Built dist\Tree Ensemble Lab.exe
pause
