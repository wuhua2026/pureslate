# ASCII-only elevate runner for bench-scan (admin MFT path, DG-1 gate retest).
# Repo path is derived from script location (<repo>\tools\diag) so this file stays
# pure ASCII: immune to PowerShell 5.1 BOM-less UTF-8 misparse (LESSONS lesson 1).
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$out = Join-Path $repo '.bench-out.txt'
Set-Content -Path $out -Value 'STARTED' -Encoding utf8
$rules = Join-Path $repo 'src-tauri\resources\rules'
& (Join-Path $repo 'src-tauri\target\release\bench-scan.exe') --rules-dir $rules 2>&1 | ForEach-Object { Add-Content -Path $out -Value $_ -Encoding utf8 }
$code = $LASTEXITCODE
Add-Content -Path $out -Value ("END_CODE=" + $code) -Encoding utf8
exit $code
