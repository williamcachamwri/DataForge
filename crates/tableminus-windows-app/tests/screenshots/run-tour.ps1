#requires -Version 5.1
<#
.SYNOPSIS
  Runs the TableMinus screenshot tour and packs the PNG captures into a zip.

.DESCRIPTION
  Launches the release binary with TABLEMINUS_SCREENSHOT_DIR set. The app walks every
  workspace view at several window sizes (see src/ui/screenshot_tour.rs), writes one PNG
  per view, then quits. The PNG files are compressed into <OutDir>\tableminus-screenshots.zip.

.EXAMPLE
  cargo build --release -p tableminus-windows-app
  .\crates\tableminus-windows-app\tests\screenshots\run-tour.ps1
#>
param(
    [string]$Exe = ".\target\release\tableminus.exe",
    [string]$OutDir = ".\target\screenshots",
    [string]$Database = "",
    [int]$TimeoutSeconds = 240,
    [switch]$PrepareDesktop
)

$ErrorActionPreference = "Stop"

if ($PrepareDesktop) {
    . (Join-Path $PSScriptRoot "desktop.ps1")
    Clear-FirstLogon
    Set-Display 1920 1080
}

if (-not (Test-Path $Exe)) { throw "Binary not found: $Exe (run cargo build --release -p tableminus-windows-app)" }
if (Get-Process tableminus -ErrorAction SilentlyContinue) { throw "Close running tableminus.exe first (single-instance lock)." }

$OutDir = [System.IO.Path]::GetFullPath($OutDir)
if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
New-Item -ItemType Directory -Path $OutDir | Out-Null

# Seed a sample SQLite database so data views have content.
if (-not $Database) {
    $Database = Join-Path $OutDir "sample.sqlite"
    $py = Get-Command python -ErrorAction SilentlyContinue
    if ($py) {
        $seed = @"
import sqlite3, sys
c = sqlite3.connect(sys.argv[1])
c.executescript('''
CREATE TABLE categories(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, description TEXT);
CREATE TABLE products(id INTEGER PRIMARY KEY, category_id INTEGER REFERENCES categories(id), name TEXT, price REAL);
''')
c.executemany('INSERT INTO categories(name, description) VALUES (?, ?)', [(f'Category {i}', f'Sample {i}') for i in range(1, 25)])
c.executemany('INSERT INTO products(category_id, name, price) VALUES (?, ?, ?)', [((i % 24) + 1, f'Product {i}', i * 1.5) for i in range(1, 200)])
c.commit()
"@
        $seed | & python - $Database
    } else {
        $Database = ""
    }
}

$env:TABLEMINUS_SCREENSHOT_DIR = $OutDir
$args = @()
if ($Database -and (Test-Path $Database)) { $args += $Database }
$proc = Start-Process -FilePath $Exe -ArgumentList $args -PassThru
if (-not $proc.WaitForExit($TimeoutSeconds * 1000)) {
    $proc.Kill()
    Write-Warning "Tour timed out after $TimeoutSeconds seconds; packing captures taken so far."
}

$pngs = Get-ChildItem $OutDir -Filter *.png -ErrorAction SilentlyContinue
if (-not $pngs) { throw "No screenshots were captured in $OutDir" }

$zip = Join-Path $OutDir "tableminus-screenshots.zip"
Compress-Archive -Path (Join-Path $OutDir "*.png") -DestinationPath $zip -Force
Write-Host ("Captured {0} screenshots -> {1}" -f $pngs.Count, $zip)
