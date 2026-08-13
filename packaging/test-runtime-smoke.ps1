$ErrorActionPreference = "Stop"

$verifyScript = Join-Path $PSScriptRoot "verify-artifacts.ps1"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ai-tool-control-runtime-smoke-test-" + [guid]::NewGuid())

$originalPath = $env:PATH
$originalPythonHome = $env:PYTHONHOME
$originalPythonPath = $env:PYTHONPATH

New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

@"
@echo off
echo AMBIENT_PYTHON_USED
exit /b 91
"@ | Set-Content -Encoding ascii (Join-Path $tempDir "python.cmd")

try {
    $env:PATH = $tempDir
    $env:PYTHONHOME = Join-Path $tempDir "poison-home"
    $env:PYTHONPATH = Join-Path $tempDir "poison-path"

    $output = @(& $verifyScript 6>&1 2>&1)

    if (-not ($output -match "PASS: bundled CPython ping smoke.")) {
        throw "BUNDLED_CPYTHON_SMOKE_NOT_CONFIRMED"
    }
}
finally {
    $env:PATH = $originalPath
    $env:PYTHONHOME = $originalPythonHome
    $env:PYTHONPATH = $originalPythonPath
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}

Write-Host "PASS: verify-artifacts proves bundled CPython works without ambient Python."