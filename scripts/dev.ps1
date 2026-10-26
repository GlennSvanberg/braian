# Loads .env, then starts the API, indexer and Vite.
# Same as `just dev`, for a Windows shell that does not have just yet.
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root

$envFile = Join-Path $root '.env'
if (Test-Path -LiteralPath $envFile) {
  Get-Content -LiteralPath $envFile -Encoding utf8 | ForEach-Object {
    $line = $_.Trim()
    if ($line.Length -eq 0 -or $line.StartsWith('#')) { return }
    $eq = $line.IndexOf('=')
    if ($eq -lt 1) { return }
    $name = $line.Substring(0, $eq).Trim()
    $value = $line.Substring($eq + 1).Trim()
    $quoted = ($value.Length -ge 2) -and (
      ($value.StartsWith('"') -and $value.EndsWith('"')) -or
      ($value.StartsWith("'") -and $value.EndsWith("'"))
    )
    if ($quoted) { $value = $value.Substring(1, $value.Length - 2) }
    Set-Item -Path "Env:$name" -Value $value
  }
}

node (Join-Path $root 'scripts/dev.mjs')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
