$ErrorActionPreference = "Stop"

$fetchScript = Join-Path $PSScriptRoot "fetch-runtimes.ps1"
$repositoryRoot = Split-Path $PSScriptRoot -Parent
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ai-tool-control-runtime-pth-test-" + [guid]::NewGuid())
$archivePath = Join-Path $tempDir "fake-runtime.zip"
$manifestPath = Join-Path $tempDir "runtime-manifest.json"
$archiveSource = Join-Path $tempDir "archive-source"
$stageDirectory = "cpython-pth-test"
$stagePath = Join-Path (Join-Path $repositoryRoot "runtimes") $stageDirectory

New-Item -ItemType Directory -Path $archiveSource -Force | Out-Null
Set-Content -Path (Join-Path $archiveSource "python.exe") -Value "fake-python"
Set-Content -Path (Join-Path $archiveSource "python314.zip") -Value "fake-stdlib"
@("python314.zip", ".", "unexpected-path", "#import site") |
    Set-Content -Encoding ascii (Join-Path $archiveSource "python314._pth")

Compress-Archive -Path (Join-Path $archiveSource "*") -DestinationPath $archivePath
$hash = (Get-FileHash -Algorithm SHA256 -Path $archivePath).Hash.ToLowerInvariant()

$manifest = @{
    schema_version = 1
    runtimes = @(
        @{
            id = "cpython"
            version = "3.14.7"
            platform = "windows"
            architecture = "x86_64"
            artifact = "fake-runtime.zip"
            url = "https://example.invalid/fake-runtime.zip"
            sha256 = $hash
            stage_directory = $stageDirectory
        }
    )
}

$manifest | ConvertTo-Json -Depth 5 |
    Set-Content -Encoding utf8 $manifestPath

try {
    & $fetchScript -ArchivePath $archivePath -ManifestPath $manifestPath
    throw "STAGING_SUCCEEDED_UNEXPECTEDLY"
}
catch {
    if ($_.Exception.Message -notmatch "isolation contents") {
        throw "Expected CPython isolation contents rejection, got: $($_.Exception.Message)"
    }
}
finally {
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $stagePath -ErrorAction SilentlyContinue
}

Write-Host "PASS: unexpected CPython isolation contents are rejected."
