[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version,

    [Parameter(Mandatory)]
    [string]$Executable,

    [Parameter(Mandatory)]
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'

function Find-WindowsSdkTool([string]$Name) {
    $sdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $tool = Get-ChildItem -Path $sdkBin -Directory |
        Sort-Object Name -Descending |
        ForEach-Object { Join-Path $_.FullName "x64\$Name" } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1

    if (-not $tool) {
        throw "$Name was not found in the Windows SDK. Install the Windows 10/11 SDK."
    }
    return $tool
}

function Invoke-SdkTool([string]$Tool, [string[]]$Arguments) {
    & $Tool @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$([System.IO.Path]::GetFileName($Tool)) failed with exit code $LASTEXITCODE."
    }
}

$projectRoot = Split-Path -Parent $PSScriptRoot
$template = Join-Path $PSScriptRoot 'AppxManifest.xml.in'
$assets = Join-Path $PSScriptRoot 'Assets'
$executablePath = [System.IO.Path]::GetFullPath((Join-Path $projectRoot $Executable))
$outputPath = [System.IO.Path]::GetFullPath((Join-Path $projectRoot $OutputDirectory))
$stagePath = Join-Path $outputPath 'stage'
$packagePath = Join-Path $outputPath "LastKey-$Version.msix"

if (-not (Test-Path -LiteralPath $executablePath -PathType Leaf)) {
    throw "Release executable was not found: $executablePath"
}

# MSIX uses four numeric version components; LastKey's build component stays zero.
$packageVersion = "$Version.0"
$manifest = (Get-Content -LiteralPath $template -Raw).Replace('@VERSION@', $packageVersion)

New-Item -ItemType Directory -Force -Path $outputPath | Out-Null
Remove-Item -LiteralPath $stagePath -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $packagePath -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stagePath | Out-Null

Set-Content -LiteralPath (Join-Path $stagePath 'AppxManifest.xml') -Value $manifest -Encoding utf8NoBOM
Copy-Item -LiteralPath $executablePath -Destination (Join-Path $stagePath 'LastKey.exe')
Copy-Item -LiteralPath $assets -Destination $stagePath -Recurse

$makePri = Find-WindowsSdkTool 'makepri.exe'
$makeAppx = Find-WindowsSdkTool 'makeappx.exe'
$priConfig = Join-Path $stagePath 'priconfig.xml'
$priFile = Join-Path $stagePath 'resources.pri'
Invoke-SdkTool $makePri @('createconfig', '/cf', $priConfig, '/dq', 'en-us')
Invoke-SdkTool $makePri @('new', '/pr', $stagePath, '/cf', $priConfig, '/of', $priFile)
Remove-Item -LiteralPath $priConfig -Force
Invoke-SdkTool $makeAppx @('pack', '/d', $stagePath, '/p', $packagePath, '/o')

Write-Output "Created $packagePath"
