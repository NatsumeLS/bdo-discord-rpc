<#
.SYNOPSIS
Refreshes assets/nodes.json and assets/territories.json from a bdo-viewer extraction.

.EXAMPLE
./scripts/world.ps1
./scripts/world.ps1 path/to/world.json
#>
#Requires -Version 7
param(
    [string]$Source = (Join-Path $env:LOCALAPPDATA "bdo-viewer\data\world.json")
)

$ErrorActionPreference = "Stop"
$world = Get-Content $Source -Raw | ConvertFrom-Json
foreach ($key in "nodes", "territories") {
    $target = Join-Path $PSScriptRoot "..\assets\$key.json"
    ConvertTo-Json -InputObject $world.$key -Depth 100 -Compress | Set-Content $target -NoNewline
    "$($world.$key.Count) $key -> $target"
}
