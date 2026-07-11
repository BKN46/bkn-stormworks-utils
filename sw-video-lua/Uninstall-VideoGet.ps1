[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$GameDir,

    [switch]$KeepVideoGet,
    [switch]$Force
)

$ErrorActionPreference = "Stop"

function Resolve-Directory {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (!(Test-Path -LiteralPath $Path -PathType Container)) {
        throw "directory not found: $Path"
    }
    return (Resolve-Path -LiteralPath $Path).ProviderPath
}

function Test-PathInside {
    param(
        [Parameter(Mandatory = $true)][string]$Child,
        [Parameter(Mandatory = $true)][string]$Parent
    )
    $childPath = [System.IO.Path]::GetFullPath($Child)
    $parentPath = [System.IO.Path]::GetFullPath($Parent).TrimEnd('\') + '\'
    return $childPath.StartsWith($parentPath, [System.StringComparison]::OrdinalIgnoreCase)
}

$gameDirPath = Resolve-Directory $GameDir
$gameOpenAl = Join-Path $gameDirPath "OpenAL64.dll"
$gameRealOpenAl = Join-Path $gameDirPath "OpenAL64_real.dll"
$targetVideoGet = Join-Path $gameDirPath "video_get"
$backupRoot = Join-Path $gameDirPath "video_get_backups"
$manifestPath = Join-Path $gameDirPath "video_get_install_manifest.json"

$running = Get-Process -Name "stormworks64" -ErrorAction SilentlyContinue
if ($running -and !$Force) {
    throw "Stormworks is running; close stormworks64.exe before uninstalling, or pass -Force if you accept the risk"
}

if (!(Test-Path -LiteralPath $gameRealOpenAl -PathType Leaf)) {
    throw "OpenAL64_real.dll was not found in $gameDirPath; the script cannot safely restore the original game DLL"
}

$manifest = $null
if (Test-Path -LiteralPath $manifestPath -PathType Leaf) {
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
}

$previousVideoGetBackup = $null
$previousManifestBackup = $null
if ($manifest) {
    $previousVideoGetBackup = [string]$manifest.previous_video_get_backup
    $previousManifestBackup = [string]$manifest.previous_manifest_backup
}

foreach ($backup in @($previousVideoGetBackup, $previousManifestBackup)) {
    if (![string]::IsNullOrWhiteSpace($backup) -and !(Test-PathInside -Child $backup -Parent $backupRoot)) {
        throw "the install manifest refers to a backup outside $backupRoot; refusing to continue"
    }
}

Write-Host "video_get uninstallation preflight passed."
Write-Host "Game directory: $gameDirPath"

if ($PSCmdlet.ShouldProcess($gameOpenAl, "replace the proxy with the original OpenAL64.dll")) {
    if (Test-Path -LiteralPath $gameOpenAl -PathType Leaf) {
        Remove-Item -LiteralPath $gameOpenAl -Force
    }
    Move-Item -LiteralPath $gameRealOpenAl -Destination $gameOpenAl
}

if (!$KeepVideoGet -and (Test-Path -LiteralPath $targetVideoGet -PathType Container)) {
    $resolvedTarget = (Resolve-Path -LiteralPath $targetVideoGet).ProviderPath
    if (!(Test-PathInside -Child $resolvedTarget -Parent $gameDirPath) -or
        (Split-Path -Leaf $resolvedTarget) -ne "video_get") {
        throw "refusing to remove unexpected directory: $resolvedTarget"
    }
    if ($PSCmdlet.ShouldProcess($resolvedTarget, "remove the installed video_get directory")) {
        Remove-Item -LiteralPath $resolvedTarget -Recurse -Force
    }
}

if (!$KeepVideoGet -and
    ![string]::IsNullOrWhiteSpace($previousVideoGetBackup) -and
    (Test-Path -LiteralPath $previousVideoGetBackup -PathType Container)) {
    if ($PSCmdlet.ShouldProcess($previousVideoGetBackup, "restore the previous video_get directory")) {
        Move-Item -LiteralPath $previousVideoGetBackup -Destination $targetVideoGet
    }
}

if (Test-Path -LiteralPath $manifestPath -PathType Leaf) {
    if ($PSCmdlet.ShouldProcess($manifestPath, "remove the video_get install manifest")) {
        Remove-Item -LiteralPath $manifestPath -Force
    }
}

if (![string]::IsNullOrWhiteSpace($previousManifestBackup) -and
    (Test-Path -LiteralPath $previousManifestBackup -PathType Leaf)) {
    if ($PSCmdlet.ShouldProcess($previousManifestBackup, "restore the previous install manifest")) {
        Copy-Item -LiteralPath $previousManifestBackup -Destination $manifestPath -Force
    }
}

if ($WhatIfPreference) {
    Write-Host "WhatIf complete; no files were changed."
} else {
    Write-Host "video_get uninstalled successfully and the original OpenAL64.dll was restored."
    if ($KeepVideoGet) {
        Write-Host "The installed video_get directory was kept."
    }
}
