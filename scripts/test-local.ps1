<#
.SYNOPSIS
    One-command local release verification suite for TableMinus (Windows).
.DESCRIPTION
    Runs rustfmt, clippy (--all-targets), all pure-Rust unit/integration/E2E suites
    with `--release -j 1` (preventing debug PDB bloat), builds `tableminus.exe`,
    and verifies `--smoke-startup`.
#>
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $RepoRoot

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)][string]$Title,
        [Parameter(Mandatory = $true)][scriptblock]$Command
    )
    Write-Host ""
    Write-Host "==> $Title" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "Step failed ($LASTEXITCODE): $Title"
    }
}

try {
    $Crates = @(
        '-p', 'tableminus-windows-platform',
        '-p', 'tableminus-state',
        '-p', 'tableminus-gfx',
        '-p', 'tableminus-services',
        '-p', 'tableminus-windows-app',
        '-p', 'tableminus-e2e'
    )

    Invoke-Step 'Checking formatting (cargo fmt --check)' {
        cargo fmt @Crates -- --check
    }

    Invoke-Step 'Running Clippy (release, all targets, -D warnings)' {
        cargo clippy --release -j 1 --no-deps --all-targets @Crates -- -D warnings
    }

    Invoke-Step 'Testing tableminus-windows-platform (unit + integration)' {
        cargo test --release -j 1 -p tableminus-windows-platform
    }

    Invoke-Step 'Testing tableminus-state (unit + state_integration)' {
        cargo test --release -j 1 -p tableminus-state
    }

    Invoke-Step 'Testing tableminus-services' {
        cargo test --release -j 1 -p tableminus-services
    }

    Invoke-Step 'Testing tableminus-windows-app (unit + all parity/integration suites)' {
        cargo test --release -j 1 -p tableminus-windows-app `
            --lib `
            --test startup_shell `
            --test layout_hit_test `
            --test keyboard_nav `
            --test live_wiring `
            --test app_model_parity `
            --test editor_and_grid_parity `
            --test workbench_parity_tools
    }

    Invoke-Step 'Testing tableminus-e2e (self-contained SQLite/DuckDB/Import/Perf suites)' {
        cargo test --release -j 1 -p tableminus-e2e `
            --test sqlite_e2e `
            --test duckdb_e2e `
            --test import_e2e `
            --test workflow_e2e `
            --test startup_perf_e2e `
            --test grid_perf_and_interop_e2e
    }

    $running = Get-Process -Name 'tableminus' -ErrorAction SilentlyContinue
    if ($running) {
        throw 'tableminus.exe is currently running; close it before building release binary.'
    }

    Invoke-Step 'Building release binary (tableminus.exe)' {
        cargo build --release -j 1 -p tableminus-windows-app
    }

    Invoke-Step 'Verifying smoke startup' {
        & .\target\release\tableminus.exe --smoke-startup
    }

    Write-Host ""
    Write-Host "All TableMinus release checks and test suites passed!" -ForegroundColor Green
}
finally {
    Pop-Location
}
