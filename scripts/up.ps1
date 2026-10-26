# Starts Postgres and Typesense.
# Same as `just up`, for a Windows shell that does not have just yet.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
docker compose up -d --wait
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
