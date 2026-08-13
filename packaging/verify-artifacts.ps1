$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path $PSScriptRoot -Parent
$manifestPath = Join-Path $PSScriptRoot "runtime-manifest.json"
$manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
$runtime = $manifest.runtimes | Where-Object { $_.id -eq "cpython" } | Select-Object -First 1

if (-not $runtime) {
    throw "CPYTHON_RUNTIME_METADATA_MISSING"
}

$runtimePath = Join-Path (Join-Path $repositoryRoot "runtimes") ([string] $runtime.stage_directory)
$pythonExe = Join-Path $runtimePath "python.exe"

if (-not (Test-Path $pythonExe -PathType Leaf)) {
    throw "BUNDLED_CPYTHON_MISSING"
}

$originalPath = $env:PATH
$originalPythonHome = $env:PYTHONHOME
$originalPythonPath = $env:PYTHONPATH

try {
    $env:PATH = ""
    $env:PYTHONHOME = $null
    $env:PYTHONPATH = $null

    $request = '{"protocol_version":1,"request_id":"smoke-1","operation":"ping","roots":[]}'

    Push-Location $runtimePath
    try {
        $output = @($request | & $pythonExe -I -m ai_tool_control_scanner 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }

    if ($exitCode -ne 0) {
        throw "BUNDLED_CPYTHON_EXITED_$exitCode"
    }

    if ($output.Count -ne 1) {
        throw "BUNDLED_CPYTHON_UNEXPECTED_OUTPUT"
    }

    try {
        $response = $output[0] | ConvertFrom-Json
    }
    catch {
        throw "BUNDLED_CPYTHON_INVALID_JSON"
    }

    if ($response.protocol_version -ne 1 -or
        $response.request_id -ne "smoke-1" -or
        $response.kind -ne "pong") {
        throw "BUNDLED_CPYTHON_PING_FAILED"
    }
}
finally {
    $env:PATH = $originalPath
    $env:PYTHONHOME = $originalPythonHome
    $env:PYTHONPATH = $originalPythonPath
}

Write-Host "PASS: bundled CPython ping smoke."