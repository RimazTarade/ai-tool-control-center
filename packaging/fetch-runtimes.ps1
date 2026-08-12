[CmdletBinding()]
param(
    [string] $ArchivePath,

    [string] $ManifestPath
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ManifestPath)) {
    $ManifestPath = Join-Path $PSScriptRoot "runtime-manifest.json"
}

$manifest = Get-Content $ManifestPath -Raw | ConvertFrom-Json
$runtime = $manifest.runtimes | Where-Object { $_.id -eq "cpython" } | Select-Object -First 1

if (-not $runtime) {
    throw "CPython runtime metadata is missing from runtime-manifest.json."
}

$downloadedArchive = $false

if ([string]::IsNullOrWhiteSpace($ArchivePath)) {
    $ArchivePath = Join-Path ([System.IO.Path]::GetTempPath()) ([string] $runtime.artifact)

    try {
        Invoke-WebRequest -Uri ([string] $runtime.url) -OutFile $ArchivePath
        $downloadedArchive = $true
    }
    catch {
        throw "Runtime download failed: $($_.Exception.Message)"
    }
}

try {
    $actualHash = (Get-FileHash -Algorithm SHA256 -Path $ArchivePath).Hash.ToLowerInvariant()
    $expectedHash = ([string] $runtime.sha256).ToLowerInvariant()

    if ($actualHash -ne $expectedHash) {
        throw "SHA-256 mismatch for CPython runtime archive."
    }

    $repositoryRoot = Split-Path $PSScriptRoot -Parent
    $runtimeRoot = Join-Path $repositoryRoot "runtimes"
    $stagePath = Join-Path $runtimeRoot ([string] $runtime.stage_directory)

    try {
        if (Test-Path $stagePath) {
            Remove-Item -Recurse -Force $stagePath
        }

        New-Item -ItemType Directory -Path $stagePath -Force | Out-Null
        Expand-Archive -Path $ArchivePath -DestinationPath $stagePath -Force

        $pythonExe = Join-Path $stagePath "python.exe"
        if (-not (Test-Path $pythonExe -PathType Leaf)) {
            throw "Invalid CPython runtime layout: python.exe is missing."
        }

        $versionParts = ([string] $runtime.version).Split(".")
        $versionTag = "$($versionParts[0])$($versionParts[1])"

        $stdlibZip = Join-Path $stagePath "python$versionTag.zip"
        if (-not (Test-Path $stdlibZip -PathType Leaf)) {
            throw "Invalid CPython runtime layout: python$versionTag.zip is missing."
        }

        $pthFileName = "python$versionTag._pth"
        $pthPath = Join-Path $stagePath $pthFileName

        if (-not (Test-Path $pthPath -PathType Leaf)) {
            throw "Invalid CPython isolation layout: $pthFileName is missing."
        }

        $activePthEntries = @(
            Get-Content $pthPath |
                ForEach-Object { $_.Trim() } |
                Where-Object { $_ -and -not $_.StartsWith("#") }
        )
        $expectedPthEntries = @("python$versionTag.zip", ".")

        if (($activePthEntries -join "`n") -ne ($expectedPthEntries -join "`n")) {
            throw "Invalid CPython isolation contents in $pthFileName."
        }

        $scannerSource = Join-Path $repositoryRoot "engine\python\src\ai_tool_control_scanner"
        if (-not (Test-Path $scannerSource -PathType Container)) {
            throw "Scanner package source is missing."
        }

        $scannerRoot = Join-Path $stagePath "scanner"
        New-Item -ItemType Directory -Path $scannerRoot -Force | Out-Null
        Copy-Item -Path $scannerSource -Destination $scannerRoot -Recurse -Force

        @(
            "python$versionTag.zip"
            "."
            "scanner"
        ) | Set-Content -Encoding ascii $pthPath
    }
    catch {
        Remove-Item -Recurse -Force $stagePath -ErrorAction SilentlyContinue
        throw
    }
}
finally {
    if ($downloadedArchive) {
        Remove-Item -Force $ArchivePath -ErrorAction SilentlyContinue
    }
}
