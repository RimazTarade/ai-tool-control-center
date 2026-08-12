$ErrorActionPreference = "Stop"

$fetchScript = Join-Path $PSScriptRoot "fetch-runtimes.ps1"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ai-tool-control-runtime-test-" + [guid]::NewGuid())
$fakeArchive = Join-Path $tempDir "python-3.14.7-embed-amd64.zip"

New-Item -ItemType Directory -Path $tempDir | Out-Null
Set-Content -Path $fakeArchive -Value "definitely-not-the-approved-runtime" -NoNewline

try {
    & $fetchScript -ArchivePath $fakeArchive
    throw "Expected runtime staging to reject an archive with the wrong SHA-256."
}
catch {
    if ($_.Exception.Message -notmatch "SHA-256 mismatch") {
        throw "Expected SHA-256 mismatch rejection, got: $($_.Exception.Message)"
    }
}
finally {
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}

Write-Host "PASS: incorrect runtime SHA-256 is rejected."
