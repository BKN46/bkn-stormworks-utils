[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$GameDir,

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

function Assert-File {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (!(Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "required file not found: $Path"
    }
}

function Get-Sha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    Assert-File $Path
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-JsonUtf8NoBom {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Path,
        [int]$Depth = 20
    )
    $json = $Value | ConvertTo-Json -Depth $Depth
    $encoding = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, $encoding)
}

$packageDir = Resolve-Directory $PSScriptRoot
$gameDirPath = Resolve-Directory $GameDir
$gameExe = Join-Path $gameDirPath "stormworks64.exe"
$gameOpenAl = Join-Path $gameDirPath "OpenAL64.dll"
$gameRealOpenAl = Join-Path $gameDirPath "OpenAL64_real.dll"
$targetVideoGet = Join-Path $gameDirPath "video_get"
$backupRoot = Join-Path $gameDirPath "video_get_backups"
$manifestPath = Join-Path $gameDirPath "video_get_install_manifest.json"

$packageProxy = Join-Path $packageDir "OpenAL64.dll"
$packageVideoGet = Join-Path $packageDir "video_get"
$packageDll = Join-Path $packageVideoGet "StormworksVideoGet.dll"
$packageRuntimeContext = Join-Path $packageVideoGet "runtime-context.json"
$packagePlugin = Join-Path $packageVideoGet "plugin.json"
$packageConfig = Join-Path $packageVideoGet "config\default.json"
$packageSignatures = Join-Path $packageVideoGet "signatures\local-dev.json"
$packageHookPlan = Join-Path $packageVideoGet "hook-plan.json"

Assert-File $gameExe
Assert-File $packageProxy
Assert-File $packageDll
Assert-File $packageRuntimeContext
Assert-File $packagePlugin
Assert-File $packageConfig
Assert-File $packageSignatures
Assert-File $packageHookPlan

$packageVideoGetPath = (Resolve-Path -LiteralPath $packageVideoGet).ProviderPath.TrimEnd('\')
$targetVideoGetPath = [System.IO.Path]::GetFullPath($targetVideoGet).TrimEnd('\')
if ($packageVideoGetPath.Equals($targetVideoGetPath, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "run this script from an extracted package outside the Stormworks game directory"
}

if (!(Test-Path -LiteralPath $gameOpenAl -PathType Leaf) -and
    !(Test-Path -LiteralPath $gameRealOpenAl -PathType Leaf)) {
    throw "neither OpenAL64.dll nor OpenAL64_real.dll exists in $gameDirPath"
}

$gameSha256 = Get-Sha256 $gameExe
$proxySha256 = Get-Sha256 $packageProxy
$plugin = Get-Content -LiteralPath $packagePlugin -Raw | ConvertFrom-Json
$signatures = Get-Content -LiteralPath $packageSignatures -Raw | ConvertFrom-Json
$hookPlan = Get-Content -LiteralPath $packageHookPlan -Raw | ConvertFrom-Json

$pluginSupportsBuild = @($plugin.game_builds | Where-Object {
    $_.sha256 -and ([string]$_.sha256).ToLowerInvariant() -eq $gameSha256
}).Count -gt 0
if (!$pluginSupportsBuild) {
    throw "this package does not support stormworks64.exe SHA-256 $gameSha256; download or build a package for this Stormworks version"
}

foreach ($declaration in @(
    @{ Name = "signatures/local-dev.json"; Hash = [string]$signatures.game_sha256 },
    @{ Name = "hook-plan.json"; Hash = [string]$hookPlan.game_sha256 }
)) {
    if ([string]::IsNullOrWhiteSpace($declaration.Hash) -or
        $declaration.Hash.ToLowerInvariant() -ne $gameSha256) {
        throw "$($declaration.Name) targets SHA-256 $($declaration.Hash), but the installed game is $gameSha256"
    }
}

if (Test-Path -LiteralPath $gameRealOpenAl -PathType Leaf) {
    if ((Get-Sha256 $gameRealOpenAl) -eq $proxySha256) {
        throw "OpenAL64_real.dll matches the video_get proxy; restore the original game DLL before installing"
    }
} elseif ((Get-Sha256 $gameOpenAl) -eq $proxySha256) {
    throw "OpenAL64.dll is already the video_get proxy, but OpenAL64_real.dll is missing; restore the original game DLL before installing"
}

$running = Get-Process -Name "stormworks64" -ErrorAction SilentlyContinue
if ($running -and !$Force) {
    throw "Stormworks is running; close stormworks64.exe before installing, or pass -Force if you accept the risk"
}

$timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMdd_HHmmss")
$previousVideoGetBackup = $null
$previousManifestBackup = $null
$createdRealOpenAl = !(Test-Path -LiteralPath $gameRealOpenAl -PathType Leaf)

Write-Host "video_get installation preflight passed."
Write-Host "Game directory: $gameDirPath"
Write-Host "Game SHA-256: $gameSha256"

if ($createdRealOpenAl -and $PSCmdlet.ShouldProcess($gameOpenAl, "rename the original OpenAL64.dll to OpenAL64_real.dll")) {
    Move-Item -LiteralPath $gameOpenAl -Destination $gameRealOpenAl
}

if (Test-Path -LiteralPath $targetVideoGet -PathType Container) {
    $previousVideoGetBackup = Join-Path $backupRoot "video_get_$timestamp"
    if ($PSCmdlet.ShouldProcess($targetVideoGet, "back up the existing video_get directory")) {
        New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null
        Move-Item -LiteralPath $targetVideoGet -Destination $previousVideoGetBackup
    }
}

if (Test-Path -LiteralPath $manifestPath -PathType Leaf) {
    $previousManifestBackup = Join-Path $backupRoot "video_get_install_manifest_$timestamp.json"
    if ($PSCmdlet.ShouldProcess($manifestPath, "back up the existing install manifest")) {
        New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null
        Copy-Item -LiteralPath $manifestPath -Destination $previousManifestBackup -Force
    }
}

if ($PSCmdlet.ShouldProcess($gameOpenAl, "install the video_get OpenAL64.dll proxy")) {
    Copy-Item -LiteralPath $packageProxy -Destination $gameOpenAl -Force
}

if ($PSCmdlet.ShouldProcess($targetVideoGet, "install the video_get plugin directory")) {
    Copy-Item -LiteralPath $packageVideoGet -Destination $targetVideoGet -Recurse -Force
}

if (!$WhatIfPreference) {
    $targetRuntimeContext = Join-Path $targetVideoGet "runtime-context.json"
    $targetPlugin = Join-Path $targetVideoGet "plugin.json"
    $targetConfig = Join-Path $targetVideoGet "config\default.json"
    $targetSignatures = Join-Path $targetVideoGet "signatures\local-dev.json"
    $targetHookPlan = Join-Path $targetVideoGet "hook-plan.json"
    $targetLogDir = Join-Path $targetVideoGet "logs"

    $runtimeContext = Get-Content -LiteralPath $targetRuntimeContext -Raw | ConvertFrom-Json
    $runtimeContext.manager_home = $targetVideoGet
    $runtimeContext.plugin_dir = $targetVideoGet
    $runtimeContext.manifest_path = $targetPlugin
    $runtimeContext.config_path = $targetConfig
    $runtimeContext.signatures_path = $targetSignatures
    $runtimeContext.hook_plan_path = $targetHookPlan
    $runtimeContext.game_exe = $gameExe
    $runtimeContext.game_sha256 = $gameSha256
    $runtimeContext.mode = "replace_dll"
    $runtimeContext.process_id = $null
    $runtimeContext.log_dir = $targetLogDir
    Write-JsonUtf8NoBom -Value $runtimeContext -Path $targetRuntimeContext

    $manifest = [ordered]@{
        schema_version = 1
        installed_at_utc = (Get-Date).ToUniversalTime().ToString("o")
        game_dir = $gameDirPath
        game_sha256 = $gameSha256
        package_dir = $packageDir
        proxy_sha256 = $proxySha256
        created_openal_real = $createdRealOpenAl
        previous_video_get_backup = $previousVideoGetBackup
        previous_manifest_backup = $previousManifestBackup
    }
    Write-JsonUtf8NoBom -Value $manifest -Path $manifestPath -Depth 8
}

if ($WhatIfPreference) {
    Write-Host "WhatIf complete; no files were changed."
} else {
    Write-Host "video_get installed successfully."
    Write-Host "Start Stormworks normally through Steam."
    Write-Host "To remove the mod, run Uninstall-VideoGet.ps1 with the same -GameDir."
}
