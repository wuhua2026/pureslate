<#
.SYNOPSIS
  PureSlate DG-1 perf-decision-gate benchmark wrapper. SPEC §8: time read-only
  traversal of temp+large+dup dimensions.
.DESCRIPTION
  Builds & runs the bench-scan binary in release under src-tauri, captures its
  stdout JSON, writes docs/verify/bench-scan.json, and PASS/FAIL on totalMs<=120000.
  First --release build links the whole Tauri lib and may take a few minutes.
.EXAMPLE
  .\tools\bench-scan.ps1
  .\tools\bench-scan.ps1 -RulesDir "src-tauri/resources/rules" -ResultFile "docs/verify/bench-scan.json"
#>
param(
    [string]$RulesDir = "src-tauri/resources/rules",
    [string]$ResultFile = "docs/verify/bench-scan.json"
)

# Continue, not Stop: cargo writes build progress to stderr, and Stop + `2>&1`
# turns each stderr line into a terminating NativeCommandError.
$ErrorActionPreference = "Continue"

$root = Split-Path -Parent $PSScriptRoot   # repo root (parent of tools/)
$srcTauri = Join-Path $root "src-tauri"
$rulesAbs = Join-Path $root $RulesDir

if (-not (Test-Path $rulesAbs)) {
    Write-Error "rules dir not found: $rulesAbs"
    exit 1
}

Write-Host ">> rules dir: $rulesAbs"
Write-Host ">> building bench-scan in --release (first build may take minutes)..."

# Run inside src-tauri; only JSON goes to stdout (progress goes to stderr -> terminal).
Push-Location $srcTauri
try {
    $stdout = & cargo run --release --bin bench-scan -- --rules-dir $rulesAbs 2>&1 | Out-String
}
finally {
    Pop-Location
}

if ($LASTEXITCODE -ne 0) {
    Write-Error "bench-scan failed (exit $LASTEXITCODE):`n$stdout"
}

# Extract the JSON body (first '{' .. last '}').
$jsonText = ""
$start = $stdout.IndexOf("```json")
if ($start -lt 0) { $start = $stdout.IndexOf('{') }
if ($start -ge 0) {
    $end = $stdout.LastIndexOf('}')
    if ($end -gt $start) {
        $jsonText = $stdout.Substring($start, $end - $start + 1)
        $jsonText = $jsonText -replace '^```json\s*', ''
    }
}
if (-not $jsonText) {
    Write-Error "could not parse JSON from bench-scan output:`n$stdout"
}

$report = $jsonText | ConvertFrom-Json

$outDir = Split-Path -Parent $ResultFile
if ($outDir) { New-Item -ItemType Directory -Force -Path $outDir | Out-Null }
$jsonText | Out-File -FilePath $ResultFile -Encoding utf8

$totalSec = [math]::Round($report.totalMs / 1000.0, 1)
Write-Host ("bench report written: {0}" -f $ResultFile)
Write-Host ("temp={0}ms  large={1}ms  dup={2}ms" -f $report.dims.temp.ms, $report.dims.large.ms, $report.dims.dup.ms)
Write-Host ("totalMs={0} ({1}s)  threshold={2}ms -> {3}" -f $report.totalMs, $totalSec, $report.thresholdMs, $(if ($report.pass) { "PASS" } else { "FAIL" }))

if (-not $report.pass) {
    Write-Host ">> NOT gate: totalMs > 120s. Must implement P1-02b (MFT read) and re-bench."
    exit 1
}
exit 0