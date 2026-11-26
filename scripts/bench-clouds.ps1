# Measures how long bringing in each scan of a corpus takes, and the most memory it takes, on Windows: the numbers in the point
# cloud PRs. As scripts/bench-clouds.sh.
#
#   scripts\bench-clouds.ps1 [-Dir target\cloud-corpus] [-Points 10000000]
param([string]$Dir = "target\cloud-corpus", [long]$Points = 10000000)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
cargo build --release -p bimcraft-cli
cargo build --release -p bimcraft-core --example cloud_corpus
if (-not (Test-Path (Join-Path $Dir "corpus.las"))) {
    & target\release\examples\cloud_corpus.exe $Dir $Points
}
"{0,-28} {1,12} {2,10} {3,12}" -f "file", "bytes", "seconds", "peak MB"
Get-ChildItem $Dir | Where-Object { $_.Extension -match '^\.(pts|xyz|ply|las|laz|e57)$' } | ForEach-Object {
    $path = $_.FullName.Replace('\', '/')
    $cmd = "point_cloud {`"path`": `"$path`", `"units`": `"m`"}"
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath target\release\bimcraft-cli.exe -ArgumentList @("run", "--cmd", "`"$($cmd.Replace('"', '\"'))`"") `
        -NoNewWindow -PassThru -RedirectStandardOutput "$env:TEMP\bench-clouds.out"
    $peak = 0
    while (-not $p.HasExited) {
        try { $p.Refresh(); $peak = [Math]::Max($peak, $p.PeakWorkingSet64) } catch {}
        Start-Sleep -Milliseconds 100
    }
    $watch.Stop()
    "{0,-28} {1,12} {2,10:N1} {3,12:N0}" -f $_.Name, $_.Length, $watch.Elapsed.TotalSeconds, ($peak / 1MB)
}
