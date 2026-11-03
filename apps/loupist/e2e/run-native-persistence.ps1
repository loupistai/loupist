[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $IsWindows -or -not [Environment]::Is64BitOperatingSystem) {
    throw "Loupist native Windows persistence requires 64-bit Windows PowerShell 7."
}

$ScriptDir = Split-Path -Parent $PSCommandPath
$Launcher = Join-Path $ScriptDir "run-native-smoke.ps1"
$StateRoot = $null

function Resolve-RealAbsoluteDirectory([string]$Path, [string]$Name) {
    if ([string]::IsNullOrWhiteSpace($Path) -or -not [System.IO.Path]::IsPathRooted($Path)) {
        throw "$Name must be an absolute directory."
    }

    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -or [bool]($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
        throw "$Name must be a real directory."
    }

    return $item.FullName
}

function Wait-ForTauriDevServerExit {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    $ownerProcessIds = @()
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $ownerProcessIds = @(
                Get-NetTCPConnection -State Listen -LocalPort 1420 -ErrorAction Stop |
                    ForEach-Object { [int]$_.OwningProcess } |
                    Select-Object -Unique
            )
        }
        catch {
            $ownerProcessIds = @()
        }
        if ($ownerProcessIds.Count -eq 0) {
            return
        }
        Start-Sleep -Milliseconds 100
    }

    throw (
        "Loupist dev server did not exit within 15 seconds " +
        "(listening PIDs: $($ownerProcessIds -join ', '))."
    )
}

try {
    if (-not (Test-Path -LiteralPath $Launcher -PathType Leaf)) {
        throw "Loupist native Windows launcher is missing: $Launcher"
    }

    $StateRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
        "loupist-persistence-e2e-" + [Guid]::NewGuid().ToString("N")
    )
    New-Item -ItemType Directory -Path $StateRoot | Out-Null
    foreach ($phase in @("seed", "restart", "copy")) {
        $PhaseRoot = Join-Path $StateRoot $(if ($phase -eq "restart") { "seed" } else { $phase })
        New-Item -ItemType Directory -Path $PhaseRoot -Force | Out-Null
        $Seed = if ($phase -eq "seed") { "1" } else { "0" }
        if ($phase -eq "copy") {
            Copy-Item -LiteralPath (Join-Path $StateRoot "seed/workspace") -Destination (Join-Path $PhaseRoot "workspace") -Recurse -Force
        }
        & $Launcher -PersistencePhase $phase -StateRoot $PhaseRoot -KnowledgeSeed $Seed
        if ($LASTEXITCODE -ne 0) { throw "Loupist native Windows persistence $phase phase failed." }
        Wait-ForTauriDevServerExit
    }

    Write-Host "Loupist native Windows persistence passed."
}
finally {
    if ($null -ne $StateRoot -and (Test-Path -LiteralPath $StateRoot)) {
        try {
            $stateItem = Get-Item -LiteralPath $StateRoot -Force -ErrorAction Stop
            if ([bool]($stateItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
                Write-Warning "Refusing to remove reparse-point persistence state: $StateRoot"
            }
            else {
                Remove-Item -LiteralPath $StateRoot -Recurse -Force -ErrorAction Stop
            }
        }
        catch {
            Write-Warning "Could not remove native Windows persistence state: $StateRoot"
        }
    }
}
