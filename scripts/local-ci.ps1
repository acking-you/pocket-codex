<#
.SYNOPSIS
Local CI gate — mirrors .github/workflows/ci.yml for the GitHub-outage
local development mode (see CLAUDE.md "Local-only development mode").

Runs the same four gates CI would: rust fmt, rust clippy, rust test, and
flutter format/analyze/test. Must be GREEN before any merge to main.

.DESCRIPTION
Differences from real CI, on purpose:
- No affected-surface detection (ci_affected.py needs python3, absent on this
  machine): Rust gates run workspace-wide. Use -SkipRust / -SkipFlutter to
  scope a run when the change is clearly one-sided; when unsure, run both.
- `cargo fmt` runs in WRITE mode instead of `--check`: on Windows, autocrlf
  makes `--check` report false "Incorrect newline style" errors for every
  file. Write mode normalizes real formatting; commit anything it changes.

.EXAMPLE
pwsh scripts/local-ci.ps1              # full gate (Rust + Flutter)
pwsh scripts/local-ci.ps1 -SkipRust    # Flutter-only change
pwsh scripts/local-ci.ps1 -SkipFlutter # Rust-only change
#>
param(
    [switch]$SkipRust,
    [switch]$SkipFlutter
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$failed = @()

function Invoke-Gate {
    param([string]$Name, [string]$WorkDir, [scriptblock]$Body)
    Write-Host "`n=== $Name ===" -ForegroundColor Cyan
    Push-Location $WorkDir
    try {
        & $Body
        if ($LASTEXITCODE -ne 0) {
            $script:failed += $Name
            Write-Host "--- $Name : FAILED (exit $LASTEXITCODE)" -ForegroundColor Red
        } else {
            Write-Host "--- $Name : ok" -ForegroundColor Green
        }
    } finally {
        Pop-Location
    }
}

if (-not $SkipRust) {
    Invoke-Gate 'rust fmt (write mode)' $repo { cargo fmt }
    Invoke-Gate 'rust clippy -D warnings' $repo {
        cargo clippy --workspace --all-targets -- -D warnings
    }
    Invoke-Gate 'rust test' $repo { cargo test --workspace }
}

if (-not $SkipFlutter) {
    $flutter = Join-Path $repo 'apps/flutter'
    Invoke-Gate 'dart format --set-exit-if-changed' $flutter {
        fvm dart format --output=none --set-exit-if-changed lib test integration_test
    }
    Invoke-Gate 'flutter analyze' $flutter { fvm flutter analyze }
    Invoke-Gate 'flutter test' $flutter { fvm flutter test }
}

Write-Host ''
if ($failed.Count -gt 0) {
    Write-Host "LOCAL CI: RED — failed gates: $($failed -join ', ')" -ForegroundColor Red
    exit 1
}
Write-Host 'LOCAL CI: GREEN — ok to merge to main.' -ForegroundColor Green
exit 0
