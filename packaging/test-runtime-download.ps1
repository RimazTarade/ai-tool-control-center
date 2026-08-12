$ErrorActionPreference = "Stop"

$fetchScript = Join-Path $PSScriptRoot "fetch-runtimes.ps1"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ai-tool-control-runtime-download-test-" + [guid]::NewGuid())
$manifestPath = Join-Path $tempDir "runtime-manifest.json"

New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

$manifest = @{
    schema_version = 1
    runtimes = @(
        @{
            id = "cpython"
            version = "3.14.7"
            platform = "windows"
            architecture = "x86_64"
            artifact = "fake-runtime.zip"
            url = "http://127.0.0.1:9/fake-runtime.zip"
            sha256 = ("0" * 64)
            stage_directory = "cpython-download-test"
        }
    )
}

$manifest | ConvertTo-Json -Depth 5 |
    Set-Content -Encoding utf8 $manifestPath

try {
    & $fetchScript -ManifestPath $manifestPath
    throw "FETCH_SUCCEEDED_UNEXPECTEDLY"
}
catch {
    if ($_.Exception.Message -notmatch "download") {
        throw "Expected runtime download attempt, got: $($_.Exception.Message)"
    }
}
finally {
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}

Write-Host "PASS: fetch mode attempts manifest-pinned runtime download."
